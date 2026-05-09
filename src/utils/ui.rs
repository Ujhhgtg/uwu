use std::sync::Arc;

use egui::{
    Color32, Context, FontDefinitions, Label, Pos2, Rect, Response, Ui, Visuals, Widget, WidgetText,
};
use egui_notify::Toasts;
use winit::window::{Fullscreen, Window};

use crate::{
    assets,
    state::{
        AppState, CanvasObject, CanvasShape, CanvasShapeType, CanvasState, CanvasStroke,
        CanvasTool, PageState, StrokeWidth, ThemeMode, WindowMode,
    },
};

pub fn apply_theme_mode_and_canvas_color(
    ctx: &Context,
    theme_mode: ThemeMode,
    canvas_color: Color32,
) {
    let is_dark = if theme_mode == ThemeMode::System {
        super::dark_mode::is_dark_mode().unwrap_or(true)
    } else {
        theme_mode == ThemeMode::Dark
    };

    if is_dark {
        // let bg_color = Visuals::dark().window_fill;
        ctx.set_visuals(Visuals {
            panel_fill: canvas_color,
            // extreme_bg_color: bg_color, // for scroll area; this also affects text input field's bg color, which is unwanted
            dark_mode: true,
            ..Visuals::dark()
        });
    } else {
        // let bg_color = Visuals::light().window_fill;
        ctx.set_visuals(Visuals {
            panel_fill: canvas_color,
            // extreme_bg_color: bg_color, // for scroll area; this also affects text input field's bg color, which is unwanted
            dark_mode: false,
            ..Visuals::light()
        });
    }
}

pub fn apply_window_mode(state: &mut AppState, window: &Arc<Window>) {
    match state.persistent.window_mode {
        WindowMode::Windowed => {
            // 窗口化
            window.set_fullscreen(None);
        }
        WindowMode::ExclusiveFullscreen => {
            // 全屏
            // 使用选中的视频模式
            if let Some(selected_index) = state.selected_video_mode_index
                && selected_index < state.fullscreen_video_modes.len()
                && let Some(mode) = state.fullscreen_video_modes.get(selected_index)
            {
                window.set_fullscreen(Some(Fullscreen::Exclusive(mode.clone())));
                return;
            }

            // 回退到第一个可用的视频模式
            window.set_fullscreen(Some(Fullscreen::Exclusive(
                state
                    .fullscreen_video_modes
                    .first()
                    .expect("no video mode available")
                    .clone(),
            )));
        }
        WindowMode::BorderlessFullscreen => {
            // 无边框全屏
            window.set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
        }
    }
}

pub enum PageAction {
    None,
    Previous,
    Next,
    New,
}

pub fn clear_interaction_state(state: &mut AppState) {
    state.selected_object_index = None;
    state.pointers.clear();
    state.shapes_inserted_count = 0;
    state.selected_shape_type = None;
}

pub fn switch_to_page_state(state: &mut AppState, page_index: usize) {
    let old = state.current_page;
    if old != page_index {
        std::mem::swap(&mut state.canvas, &mut state.pages[old].canvas);
        std::mem::swap(&mut state.history, &mut state.pages[old].history);
        state.current_page = page_index;
        std::mem::swap(&mut state.canvas, &mut state.pages[page_index].canvas);
        std::mem::swap(&mut state.history, &mut state.pages[page_index].history);
    }
    state.view_offset = Default::default();
    state.view_zoom = 1.0;
    clear_interaction_state(state);
}

pub fn add_new_page_state(state: &mut AppState) {
    let old = state.current_page;
    state.pages[old].canvas = std::mem::take(&mut state.canvas);
    state.pages[old].history = std::mem::take(&mut state.history);
    state.pages.push(PageState::default());
    let new_idx = state.pages.len() - 1;
    state.current_page = new_idx;
    state.view_offset = Default::default();
    state.view_zoom = 1.0;
    clear_interaction_state(state);
}

pub fn load_canvas_from_file(state: &mut AppState) {
    match CanvasState::load_from_file_with_dialog() {
        Ok(canvas) => {
            add_new_page_state(state);
            state.canvas = canvas;
            state.show_welcome_window = false;
            state.toasts.success("成功加载画布!");
        }
        Err(err) => {
            state.toasts.error(format!("画布加载失败: {}!", err));
        }
    };
}

pub fn save_canvas_to_file(toasts: &mut Toasts, canvas: &CanvasState) {
    match canvas.save_to_file_with_dialog() {
        Ok(_) => {
            toasts.success("成功保存画布!");
        }
        Err(err) => {
            toasts.error(format!("画布保存失败: {}!", err));
        }
    }
}

pub fn setup_fonts(ctx: &mut Context) {
    let mut fonts = FontDefinitions::default();

    let font_bytes = assets::font_bytes();
    let font_name = "cjk_font";
    fonts.font_data.insert(
        font_name.to_owned(),
        Arc::new(egui::FontData::from_owned(font_bytes.to_vec())),
    );

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push(font_name.to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push(font_name.to_owned());

    ctx.set_fonts(fonts);
}

pub trait UiExtras {
    fn my_label(&mut self, text: impl Into<WidgetText>) -> Response;
}

impl UiExtras for Ui {
    #[inline(always)]
    fn my_label(&mut self, text: impl Into<WidgetText>) -> Response {
        Label::new(text).selectable(false).ui(self)
    }
}

pub fn create_shape_object(
    state: &mut AppState,
    shape_type: CanvasShapeType,
    start_pos: Pos2,
    end_pos: Pos2,
) {
    match shape_type {
        CanvasShapeType::Line => {
            let stroke = CanvasStroke {
                points: vec![start_pos, end_pos],
                width: StrokeWidth::Fixed(3.0),
                color: Color32::WHITE,
                base_width: 3.0,
                shape: Some(CanvasShapeType::Line),
            };
            let idx = state.canvas.objects.len();
            state
                .history
                .save_add_object(idx, CanvasObject::Stroke(stroke.clone()));
            state.canvas.objects.push(CanvasObject::Stroke(stroke));
        }
        CanvasShapeType::Arrow => {
            let len = start_pos.distance(end_pos);
            if len > 1.0 {
                let dir = (end_pos - start_pos) / len;
                let arrow_size = (len * 0.15).max(10.0);
                let angle = 30.0_f32.to_radians();
                let cos = angle.cos();
                let sin = angle.sin();
                let left_dir = egui::vec2(dir.x * cos - dir.y * sin, dir.x * sin + dir.y * cos);
                let right_dir = egui::vec2(dir.x * cos + dir.y * sin, -dir.x * sin + dir.y * cos);

                let stroke = CanvasStroke {
                    points: vec![
                        start_pos,
                        end_pos,
                        end_pos - left_dir * arrow_size,
                        end_pos,
                        end_pos - right_dir * arrow_size,
                        end_pos,
                    ],
                    width: StrokeWidth::Fixed(3.0),
                    color: Color32::WHITE,
                    base_width: 3.0,
                    shape: Some(CanvasShapeType::Arrow),
                };
                let idx = state.canvas.objects.len();
                state
                    .history
                    .save_add_object(idx, CanvasObject::Stroke(stroke.clone()));
                state.canvas.objects.push(CanvasObject::Stroke(stroke));
            }
        }
        CanvasShapeType::Rectangle => {
            let rect = Rect::from_two_pos(start_pos, end_pos);
            let stroke = CanvasStroke {
                points: vec![
                    rect.min,
                    egui::pos2(rect.max.x, rect.min.y),
                    rect.max,
                    egui::pos2(rect.min.x, rect.max.y),
                    rect.min,
                ],
                width: StrokeWidth::Fixed(3.0),
                color: Color32::WHITE,
                base_width: 3.0,
                shape: Some(CanvasShapeType::Rectangle),
            };
            let idx = state.canvas.objects.len();
            state
                .history
                .save_add_object(idx, CanvasObject::Stroke(stroke.clone()));
            state.canvas.objects.push(CanvasObject::Stroke(stroke));
        }
        CanvasShapeType::Triangle => {
            let rect = Rect::from_two_pos(start_pos, end_pos);
            let size = rect.width().max(rect.height());
            let tl = rect.min;
            let p1 = tl + egui::vec2(size / 2.0, 0.0);
            let p2 = tl + egui::vec2(size, size);
            let p3 = tl + egui::vec2(0.0, size);
            let stroke = CanvasStroke {
                points: vec![p1, p2, p3, p1],
                width: StrokeWidth::Fixed(3.0),
                color: Color32::WHITE,
                base_width: 3.0,
                shape: Some(CanvasShapeType::Triangle),
            };
            let idx = state.canvas.objects.len();
            state
                .history
                .save_add_object(idx, CanvasObject::Stroke(stroke.clone()));
            state.canvas.objects.push(CanvasObject::Stroke(stroke));
        }
        CanvasShapeType::Circle => {
            let center = start_pos + (end_pos - start_pos) / 2.0;
            let size = start_pos.distance(end_pos);
            let shape = CanvasShape {
                shape_type,
                pos: center,
                size,
                color: Color32::WHITE,
            };
            let idx = state.canvas.objects.len();
            state
                .history
                .save_add_object(idx, CanvasObject::Shape(shape.clone()));
            state.canvas.objects.push(CanvasObject::Shape(shape));
        }
    }

    state.shapes_inserted_count += 1;
    if !state.continuous_insert && state.shapes_inserted_count >= 1 {
        state.current_tool = CanvasTool::Brush;
    }
}
