use std::time::Instant;

use egui::Pos2;

use crate::state::{
    ActiveStroke, AppState, CanvasObject, CanvasStroke, DynamicBrushWidthMode, PointerInteraction,
    PointerState, StrokeWidth,
};

#[cfg_attr(feature = "profiling", profiling::function)]
pub fn brush_stroke_start(state: &mut AppState, pointer_id: u64, pos: Pos2, pressure: Option<f32>) {
    let start_time = Instant::now();
    let width = super::calculate_dynamic_width(
        state.brush_width,
        state.dynamic_brush_width_mode,
        0,
        1,
        None,
        pressure,
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
                    pressures: vec![pressure.unwrap_or(0.0)],
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
    pressure: Option<f32>,
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

                    // Keep pressure samples aligned with the straightened points.
                    if active_stroke.pressures.len() >= 2 {
                        let first_pressure = active_stroke.pressures[0];
                        let last_pressure = *active_stroke.pressures.last().unwrap();
                        active_stroke.pressures = if active_stroke.points.len() == 1 {
                            vec![first_pressure]
                        } else {
                            vec![first_pressure, last_pressure]
                        };
                    }
                }
            }
            active_stroke.last_movement_time = Instant::now();
        }
    }

    // Sample in screen space: a fixed canvas-space threshold would add far too
    // few points when zoomed in and too many when zoomed out.
    let screen_distance = active_stroke.points.last().unwrap().distance(pos) * state.view_zoom;
    if active_stroke.points.is_empty() || screen_distance > 1.0 {
        let speed = if !active_stroke.points.is_empty() && !active_stroke.times.is_empty() {
            let last_time = active_stroke.times.last().unwrap();
            let time_delta = ((current_time - last_time) as f32).max(0.001);
            // Speed-based width is defined in screen pixels per second.
            Some(screen_distance / time_delta)
        } else {
            None
        };

        active_stroke.points.push(pos);
        active_stroke.times.push(current_time);
        active_stroke.pressures.push(pressure.unwrap_or(0.0));

        if state.dynamic_brush_width_mode != DynamicBrushWidthMode::Disabled {
            let stroke_width = super::calculate_dynamic_width(
                state.brush_width,
                state.dynamic_brush_width_mode,
                active_stroke.points.len() - 1,
                active_stroke.points.len(),
                speed,
                pressure,
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
        cached_bbox: std::cell::Cell::new(None),
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Pos2;

    #[test]
    fn test_apply_stroke_smoothing_few_points() {
        let points = vec![Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)];
        let smoothed = apply_stroke_smoothing(&points);
        assert_eq!(smoothed.len(), 2);
        assert_eq!(smoothed[0], Pos2::new(0.0, 0.0));
        assert_eq!(smoothed[1], Pos2::new(1.0, 1.0));
    }

    #[test]
    fn test_apply_stroke_smoothing_many_points() {
        let mut points = Vec::new();
        for i in 0..100 {
            points.push(Pos2::new(i as f32, i as f32));
        }
        let smoothed = apply_stroke_smoothing(&points);
        assert!(smoothed.len() >= 2);
        assert_eq!(smoothed.first().unwrap(), &Pos2::new(0.0, 0.0));
        // The exact coordinates of the last point may vary due to moving average and corner cutting.
        let last = smoothed.last().unwrap();
        assert!(last.x > 90.0 && last.y > 90.0);
    }
}
