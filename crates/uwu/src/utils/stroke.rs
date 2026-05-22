use std::time::Instant;

use egui::Pos2;

use crate::state::{
    ActiveStroke, AppState, CanvasObject, CanvasStroke, DynamicBrushWidthMode, PointerInteraction,
    PointerState, StrokeWidth,
};

#[cfg_attr(feature = "profiling", profiling::function)]
pub fn brush_stroke_start(state: &mut AppState, pointer_id: u64, pos: Pos2) {
    let start_time = Instant::now();
    let width = super::calculate_dynamic_width(
        state.brush_width,
        state.dynamic_brush_width_mode,
        0,
        1,
        None,
    );
    state.pointers.insert(
        pointer_id,
        PointerState {
            id: pointer_id,
            pos,
            prev_pos: None,
            interaction: PointerInteraction::Drawing {
                active_stroke: ActiveStroke {
                    points: vec![pos],
                    width,
                    times: vec![0.0],
                    start_time,
                    last_movement_time: start_time,
                },
            },
        },
    );
}

#[cfg_attr(feature = "profiling", profiling::function)]
pub fn brush_stroke_add_point(
    state: &mut AppState,
    pointer_id: u64,
    pos: Pos2,
    apply_straightening: bool,
) {
    let Some(pointer) = state.pointers.get_mut(&pointer_id) else {
        return;
    };
    pointer.pos = pos;
    let PointerInteraction::Drawing { active_stroke } = &mut pointer.interaction else {
        return;
    };

    let current_time = active_stroke.start_time.elapsed().as_secs_f64();

    if apply_straightening && state.persistent.stroke_straightening {
        let time_since_last_movement = active_stroke.last_movement_time.elapsed().as_secs_f32();
        if time_since_last_movement > 0.5 {
            let straightened_points = super::straighten_stroke(
                &active_stroke.points,
                state.persistent.stroke_straightening_tolerance,
            );
            if straightened_points.len() != active_stroke.points.len() {
                let has_dynamic_mode =
                    state.dynamic_brush_width_mode != DynamicBrushWidthMode::Disabled;
                active_stroke.points = straightened_points;
                if let StrokeWidth::Dynamic(v) = &active_stroke.width
                    && !v.is_empty()
                {
                    let first_width = v[0];
                    let last_width = *v.last().unwrap();
                    active_stroke.width = if active_stroke.points.len() == 1 && !has_dynamic_mode {
                        StrokeWidth::Fixed(first_width)
                    } else {
                        StrokeWidth::Dynamic(vec![first_width, last_width])
                    };
                }
            }
            active_stroke.last_movement_time = Instant::now();
        }
    }

    if active_stroke.points.is_empty() || active_stroke.points.last().unwrap().distance(pos) > 1.0 {
        let speed = if !active_stroke.points.is_empty() && !active_stroke.times.is_empty() {
            let last_time = active_stroke.times.last().unwrap();
            let time_delta = ((current_time - last_time) as f32).max(0.001);
            let distance = active_stroke.points.last().unwrap().distance(pos);
            Some(distance / time_delta)
        } else {
            None
        };

        active_stroke.points.push(pos);
        active_stroke.times.push(current_time);

        if state.dynamic_brush_width_mode != DynamicBrushWidthMode::Disabled {
            let stroke_width = super::calculate_dynamic_width(
                state.brush_width,
                state.dynamic_brush_width_mode,
                active_stroke.points.len() - 1,
                active_stroke.points.len(),
                speed,
            );
            active_stroke.width.push(stroke_width.first());
        }

        active_stroke.last_movement_time = Instant::now();
    }
}

#[cfg_attr(feature = "profiling", profiling::function)]
pub fn brush_stroke_end(state: &mut AppState, pointer_id: u64) {
    // Validate stroke before removing
    let valid = state
        .pointers
        .get(&pointer_id)
        .is_some_and(|p| match &p.interaction {
            PointerInteraction::Drawing { active_stroke } => {
                if let StrokeWidth::Dynamic(v) = &active_stroke.width {
                    v.len() == active_stroke.points.len()
                } else {
                    true
                }
            }
            _ => false,
        });

    if !valid {
        state.pointers.remove(&pointer_id);
        return;
    }

    let Some(pointer) = state.pointers.remove(&pointer_id) else {
        return;
    };
    let PointerInteraction::Drawing { active_stroke } = pointer.interaction else {
        unreachable!()
    };

    let mut final_points = if state.persistent.stroke_smoothing {
        apply_stroke_smoothing(&active_stroke.points)
    } else {
        active_stroke.points
    };

    let width = super::apply_point_interpolation_in_place(
        &mut final_points,
        &active_stroke.width,
        state.persistent.interpolation_frequency,
    );

    let new_stroke = CanvasStroke {
        points: final_points,
        width,
        color: state.brush_color,
        base_width: state.brush_width,
        shape: None,
    };
    let index = state.canvas.objects.len();
    state
        .history
        .save_add_object(index, CanvasObject::Stroke(new_stroke.clone()));
    state.canvas.objects.push(CanvasObject::Stroke(new_stroke));
}

#[must_use]
#[cfg_attr(feature = "profiling", profiling::function)]
fn apply_stroke_smoothing(points: &[Pos2]) -> Vec<Pos2> {
    if points.len() < 3 {
        return points.to_vec();
    }

    // -----------------------------
    // 1. Distance-based resampling
    // -----------------------------
    let target_spacing = 2.0; // pixels; tune for device DPI
    let mut resampled = Vec::with_capacity(points.len());

    resampled.push(points[0]);
    let mut acc_dist = 0.0;

    for i in 1..points.len() {
        let prev = points[i - 1];
        let curr = points[i];
        let dx = curr.x - prev.x;
        let dy = curr.y - prev.y;
        let dist = (dx * dx + dy * dy).sqrt();

        acc_dist += dist;

        if acc_dist >= target_spacing {
            resampled.push(curr);
            acc_dist = 0.0;
        }
    }

    if resampled.len() < 3 {
        return resampled;
    }

    // --------------------------------
    // 2. Chaikin corner cutting
    // --------------------------------
    let mut smoothed = resampled;

    let iterations = 2; // 2–3 recommended for real-time strokes

    for _ in 0..iterations {
        let mut next = Vec::with_capacity(smoothed.len() * 2);
        next.push(smoothed[0]);

        for i in 0..smoothed.len() - 1 {
            let p0 = smoothed[i];
            let p1 = smoothed[i + 1];

            let q = Pos2 {
                x: 0.75 * p0.x + 0.25 * p1.x,
                y: 0.75 * p0.y + 0.25 * p1.y,
            };
            let r = Pos2 {
                x: 0.25 * p0.x + 0.75 * p1.x,
                y: 0.25 * p0.y + 0.75 * p1.y,
            };

            next.push(q);
            next.push(r);
        }

        next.push(*smoothed.last().unwrap());
        smoothed = next;
    }

    // --------------------------------
    // 3. Light moving-average cleanup
    // --------------------------------
    let len = smoothed.len();
    let mut final_points = Vec::with_capacity(len);

    if len > 0 {
        final_points.push(smoothed[0]);
    }

    for i in 1..smoothed.len() - 1 {
        final_points.push(Pos2 {
            x: (smoothed[i - 1].x + smoothed[i].x + smoothed[i + 1].x) / 3.0,
            y: (smoothed[i - 1].y + smoothed[i].y + smoothed[i + 1].y) / 3.0,
        });
    }

    if len > 1 {
        final_points.push(smoothed[len - 1]);
    }

    final_points
}
