pub mod flat;

use egui::{Color32, Pos2, Stroke};
use egui_notify::Toasts;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use wgpu::Backend;
use wgpu::PresentMode;
use winit::dpi::PhysicalPosition;

#[cfg(feature = "startup_animation")]
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
#[cfg(feature = "startup_animation")]
use rodio::Decoder;
#[cfg(feature = "startup_animation")]
use rodio::DeviceSinkBuilder;
#[cfg(feature = "startup_animation")]
use rodio::Player;
#[cfg(feature = "startup_animation")]
use std::io::Cursor;

use crate::utils;

/// Magic header for canvas files: `b"UWU"` followed by format version byte.
/// Must be kept in sync with [`CANVAS_FILE_HEADER`].
pub(crate) const CANVAS_FILE_MAGIC: &[u8; 3] = b"UWU";
pub(crate) const CANVAS_FILE_VERSION: u8 = 3;
pub(crate) const CANVAS_FILE_EXT: &str = "owo"; // open whiteboard objects

pub(crate) fn make_canvas_file_header() -> [u8; 4] {
    let mut h = [0u8; 4];
    h[..3].copy_from_slice(CANVAS_FILE_MAGIC);
    h[3] = CANVAS_FILE_VERSION;
    h
}

/// Dynamic brush width mode for stroke rendering
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DynamicBrushWidthMode {
    #[default]
    Disabled, // No dynamic width adjustment
    BrushTip,   // Simulates brush tip pressure for calligraphy effect
    SpeedBased, // Adjusts width based on drawing speed
}

/// Stroke width representation
#[derive(Debug, Clone)]
pub enum StrokeWidth {
    Fixed(f32),
    Dynamic(Vec<f32>),
}

impl StrokeWidth {
    pub fn get(&self, index: usize) -> f32 {
        match self {
            StrokeWidth::Fixed(w) => *w,
            StrokeWidth::Dynamic(v) => v[index],
        }
    }

    pub fn first(&self) -> f32 {
        match self {
            StrokeWidth::Fixed(w) => *w,
            StrokeWidth::Dynamic(v) => v[0],
        }
    }

    pub fn last(&self) -> f32 {
        match self {
            StrokeWidth::Fixed(w) => *w,
            StrokeWidth::Dynamic(v) => *v.last().unwrap(),
        }
    }

    #[cfg_attr(feature = "profiling", profiling::function)]
    pub fn max_width(&self) -> f32 {
        match self {
            StrokeWidth::Fixed(w) => *w,
            StrokeWidth::Dynamic(v) => v.iter().copied().fold(0.0, f32::max),
        }
    }

    pub fn push(&mut self, width: f32) {
        match self {
            StrokeWidth::Fixed(w) => {
                if (*w - width).abs() >= 0.01 {
                    *self = StrokeWidth::Dynamic(vec![*w, width]);
                }
            }
            StrokeWidth::Dynamic(v) => v.push(width),
        }
    }

    pub fn len(&self) -> Option<usize> {
        match self {
            StrokeWidth::Fixed(_) => None,
            StrokeWidth::Dynamic(v) => Some(v.len()),
        }
    }
}

impl From<f32> for StrokeWidth {
    fn from(width: f32) -> Self {
        StrokeWidth::Fixed(width)
    }
}

impl From<Vec<f32>> for StrokeWidth {
    fn from(widths: Vec<f32>) -> Self {
        if widths.is_empty() {
            return StrokeWidth::Fixed(0.0);
        }
        let first = widths[0];
        if widths.iter().all(|w| (w - first).abs() < 0.01) {
            StrokeWidth::Fixed(first)
        } else {
            StrokeWidth::Dynamic(widths)
        }
    }
}

/// Transform handle types for object manipulation (resize)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransformHandle {
    // 8 resize handles around the bounding box
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// Available tools for canvas interaction
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasTool {
    Passthrough, // Only available in passthrough mode; passes clicks through to underlying windows
    Select,      // Select and manipulate objects
    #[default]
    Brush, // Draw freehand strokes
    View,        // Move/zoom the canvas view
    ObjectEraser, // Delete entire objects
    PixelEraser, // Erase pixel by pixel
    Insert,      // Insert images, text, or shapes
    Settings,    // Open settings panel
}

/// Tabs within the Insert tool
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum InsertTab {
    #[default]
    Shape,
    Text,
    Image,
}

/// Trait for objects that can be rendered on the canvas
pub trait CanvasObjectOps {
    /// Renders the object using the provided painter, transformed by view_offset and zoom
    fn paint(&self, painter: &egui::Painter, selected: bool, view_offset: egui::Vec2, zoom: f32);
    /// Returns the axis-aligned bounding rectangle of the object
    fn bounding_box(&self) -> egui::Rect;
    /// Transforms the object using the specified handle and drag parameters
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        drag_start: Pos2,
        current_pos: Pos2,
    );
}

/// Image object that can be placed on the canvas
#[derive(Clone)]
pub struct CanvasImage {
    pub texture: egui::TextureHandle,
    pub pos: Pos2,
    pub size: egui::Vec2,
    pub aspect_ratio: f32,
    pub marked_for_deletion: bool, // Deferred deletion to avoid panic due to texture in use
    pub image_data: Arc<[u8]>,     // RGBA pixel data for export
    pub image_size: [u32; 2],      // [width, height] of the original image
}

impl CanvasObjectOps for CanvasImage {
    /// Transforms the image based on the dragged handle
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn transform(
        &mut self,
        handle: TransformHandle,
        _delta: egui::Vec2,
        _drag_start: Pos2,
        current_pos: Pos2,
    ) {
        let bbox = self.bounding_box();

        match handle {
            TransformHandle::TopLeft => {
                let new_min = current_pos;
                let new_max = bbox.max;
                let new_size = egui::vec2(
                    (new_max.x - new_min.x).max(10.0),
                    (new_max.y - new_min.y).max(10.0),
                );
                self.size = new_size;
                self.pos = new_min;
            }
            TransformHandle::Top => {
                let new_height = (bbox.max.y - current_pos.y).max(10.0);
                self.size.y = new_height;
                self.pos.y = current_pos.y;
            }
            TransformHandle::TopRight => {
                let new_max = Pos2::new(current_pos.x, bbox.max.y);
                let new_min = Pos2::new(bbox.min.x, current_pos.y);
                let new_size = egui::vec2(
                    (new_max.x - new_min.x).max(10.0),
                    (new_max.y - new_min.y).max(10.0),
                );
                self.size = new_size;
                self.pos.y = new_min.y;
            }
            TransformHandle::Left => {
                let new_width = (bbox.max.x - current_pos.x).max(10.0);
                self.size.x = new_width;
                self.pos.x = current_pos.x;
            }
            TransformHandle::Right => {
                let new_width = (current_pos.x - bbox.min.x).max(10.0);
                self.size.x = new_width;
            }
            TransformHandle::BottomLeft => {
                let new_min = Pos2::new(current_pos.x, bbox.min.y);
                let new_max = Pos2::new(bbox.max.x, current_pos.y);
                let new_size = egui::vec2(
                    (new_max.x - new_min.x).max(10.0),
                    (new_max.y - new_min.y).max(10.0),
                );
                self.size = new_size;
                self.pos.x = new_min.x;
            }
            TransformHandle::Bottom => {
                let new_height = (current_pos.y - bbox.min.y).max(10.0);
                self.size.y = new_height;
            }
            TransformHandle::BottomRight => {
                let new_size = egui::vec2(
                    (current_pos.x - bbox.min.x).max(10.0),
                    (current_pos.y - bbox.min.y).max(10.0),
                );
                self.size = new_size;
            }
        }
    }

    /// Returns the bounding rectangle of the image
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn bounding_box(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, self.size)
    }

    /// Renders the image on the canvas, drawing selection UI if selected
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn paint(&self, painter: &egui::Painter, selected: bool, view_offset: egui::Vec2, zoom: f32) {
        let img_rect = self.bounding_box().translate(-view_offset);
        let img_rect = egui::Rect::from_min_size(img_rect.min * zoom, img_rect.size() * zoom);
        painter.image(
            self.texture.id(),
            img_rect,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // Draw selection border and resize handles when selected
        if selected {
            painter.rect_stroke(
                img_rect,
                0.0,
                Stroke::new(2.0_f32, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, img_rect);
        }
    }
}

impl fmt::Debug for CanvasImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanvasImage")
            .field("texture", &"<TextureHandle>")
            .field("pos", &self.pos)
            .field("size", &self.size)
            .field("aspect_ratio", &self.aspect_ratio)
            .field("marked_for_deletion", &self.marked_for_deletion)
            .field("image_size", &self.image_size)
            .finish()
    }
}

/// Text object that can be placed on the canvas
#[derive(Debug, Clone)]
pub struct CanvasText {
    pub text: String,
    pub pos: Pos2,
    pub color: Color32,
    pub font_size: f32,
    pub cached_size: Option<egui::Vec2>,
}

impl CanvasObjectOps for CanvasText {
    /// Transforms the text object, scaling font size for resize handles
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        _drag_start: Pos2,
        _current_pos: Pos2,
    ) {
        match handle {
            TransformHandle::TopLeft
            | TransformHandle::Top
            | TransformHandle::TopRight
            | TransformHandle::Left
            | TransformHandle::Right
            | TransformHandle::BottomLeft
            | TransformHandle::Bottom
            | TransformHandle::BottomRight => {
                let scale_factor = 1.0 + (delta.x + delta.y) / 200.0;
                self.font_size = (self.font_size * scale_factor).max(6.0);
                self.cached_size = None;
            }
        }
    }

    /// Returns the bounding rectangle for the text
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn bounding_box(&self) -> egui::Rect {
        if let Some(size) = self.cached_size {
            egui::Rect::from_min_size(self.pos, size)
        } else {
            let approx_char_width = self.font_size * 0.6;
            let approx_width = self.text.len() as f32 * approx_char_width;
            let approx_height = self.font_size * 1.2;
            egui::Rect::from_min_size(self.pos, egui::vec2(approx_width, approx_height))
        }
    }

    /// Renders the text on the canvas with optional selection UI
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn paint(&self, painter: &egui::Painter, selected: bool, view_offset: egui::Vec2, zoom: f32) {
        let pos = (self.pos - view_offset) * zoom;
        let zoomed_font_size = self.font_size * zoom;
        let text_galley = painter.layout_no_wrap(
            self.text.clone(),
            egui::FontId::proportional(zoomed_font_size),
            self.color,
        );
        let text_shape = egui::epaint::TextShape {
            pos,
            galley: text_galley.clone(),
            underline: egui::Stroke::NONE,
            override_text_color: None,
            angle: 0.0,
            fallback_color: self.color,
            opacity_factor: 1.0,
        };
        painter.add(text_shape);

        if selected {
            let text_rect = self.bounding_box().translate(-view_offset);
            let text_rect =
                egui::Rect::from_min_size(text_rect.min * zoom, text_rect.size() * zoom);
            painter.rect_stroke(
                text_rect,
                0.0,
                Stroke::new(2.0_f32, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, text_rect);
        }
    }
}

/// Available shape types for the canvas
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasShapeType {
    Line,
    Arrow,
    Rectangle,
    Triangle,
    Circle,
}

/// Shape object that can be placed on the canvas
#[derive(Debug, Clone)]
pub struct CanvasShape {
    pub shape_type: CanvasShapeType,
    pub pos: Pos2,
    pub size: f32,
    pub color: Color32,
}

impl CanvasObjectOps for CanvasShape {
    /// Transforms the shape, scaling uniformly for resize handles
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        _drag_start: Pos2,
        _current_pos: Pos2,
    ) {
        match handle {
            TransformHandle::TopLeft
            | TransformHandle::Top
            | TransformHandle::TopRight
            | TransformHandle::Left
            | TransformHandle::Right
            | TransformHandle::BottomLeft
            | TransformHandle::Bottom
            | TransformHandle::BottomRight => {
                // Scale the shape size uniformly
                let scale_factor = 1.0 + (delta.x + delta.y) / 200.0;
                self.size = (self.size * scale_factor).max(10.0);
            }
        }
    }

    /// Returns the bounding rectangle of the shape with padding for handles
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn bounding_box(&self) -> egui::Rect {
        match self.shape_type {
            CanvasShapeType::Line => {
                let end_point = Pos2::new(self.pos.x + self.size, self.pos.y);
                let min_x = self.pos.x.min(end_point.x) - 5.0;
                let max_x = self.pos.x.max(end_point.x) + 5.0;
                let min_y = self.pos.y.min(end_point.y) - 5.0;
                let max_y = self.pos.y.max(end_point.y) + 5.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            CanvasShapeType::Arrow => {
                let end_point = Pos2::new(self.pos.x + self.size, self.pos.y);
                let min_x = self.pos.x.min(end_point.x) - 5.0;
                let max_x = self.pos.x.max(end_point.x) + 5.0;
                let min_y = self.pos.y.min(end_point.y) - 15.0;
                let max_y = self.pos.y.max(end_point.y) + 15.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            CanvasShapeType::Rectangle => {
                egui::Rect::from_min_size(self.pos, egui::vec2(self.size, self.size))
            }
            CanvasShapeType::Triangle => {
                let half_size = self.size / 2.0;
                let min_x = self.pos.x - 5.0;
                let max_x = self.pos.x + self.size + 5.0;
                let min_y = self.pos.y - 5.0;
                let max_y = self.pos.y + half_size + 5.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            CanvasShapeType::Circle => {
                let radius = self.size / 2.0;
                egui::Rect::from_min_max(
                    Pos2::new(self.pos.x - radius - 5.0, self.pos.y - radius - 5.0),
                    Pos2::new(self.pos.x + radius + 5.0, self.pos.y + radius + 5.0),
                )
            }
        }
    }

    /// Renders the shape and optional selection UI
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn paint(&self, painter: &egui::Painter, selected: bool, view_offset: egui::Vec2, zoom: f32) {
        let p = (self.pos - view_offset) * zoom;
        let z_size = self.size * zoom;
        let stroke = Stroke::new(2.0_f32 * zoom, self.color);
        // Draw the shape itself
        match self.shape_type {
            CanvasShapeType::Line => {
                let end_point = Pos2::new(p.x + z_size, p.y);
                painter.line_segment([p, end_point], stroke);
            }
            CanvasShapeType::Arrow => {
                let end_point = Pos2::new(p.x + z_size, p.y);
                painter.line_segment([p, end_point], stroke);

                // 绘制箭头头部
                let arrow_size = z_size * 0.1;
                let arrow_angle = std::f32::consts::PI / 6.0; // 30度
                let arrow_point1 = Pos2::new(
                    end_point.x - arrow_size * arrow_angle.cos(),
                    end_point.y - arrow_size * arrow_angle.sin(),
                );
                let arrow_point2 = Pos2::new(
                    end_point.x - arrow_size * arrow_angle.cos(),
                    end_point.y + arrow_size * arrow_angle.sin(),
                );

                painter.line_segment([end_point, arrow_point1], stroke);
                painter.line_segment([end_point, arrow_point2], stroke);
            }
            CanvasShapeType::Rectangle => {
                let rect = egui::Rect::from_min_size(p, egui::vec2(z_size, z_size));
                painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Outside);
            }
            CanvasShapeType::Triangle => {
                let half_size = z_size / 2.0;
                let points = [
                    p,
                    Pos2::new(p.x + z_size, p.y),
                    Pos2::new(p.x + half_size, p.y + half_size),
                ];
                painter.add(egui::Shape::convex_polygon(
                    points.to_vec(),
                    self.color,
                    stroke,
                ));
            }
            CanvasShapeType::Circle => {
                painter.circle_stroke(p, z_size / 2.0, stroke);
            }
        }

        // Draw selection border and resize handles when selected
        if selected {
            let shape_rect = self.bounding_box().translate(-view_offset);
            let shape_rect =
                egui::Rect::from_min_size(shape_rect.min * zoom, shape_rect.size() * zoom);
            painter.rect_stroke(
                shape_rect,
                0.0,
                Stroke::new(2.0_f32, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, shape_rect);
        }
    }
}

/// Enum representing all possible canvas object types
#[derive(Debug, Clone)]
pub enum CanvasObject {
    Stroke(CanvasStroke),
    Image(CanvasImage),
    Text(CanvasText),
    Shape(CanvasShape),
}

impl CanvasObject {
    /// Moves an object by the specified delta vector
    #[cfg_attr(feature = "profiling", profiling::function)]
    pub fn move_object(object: &mut CanvasObject, delta: egui::Vec2) {
        match object {
            CanvasObject::Image(img) => {
                img.pos += delta;
            }
            CanvasObject::Text(text) => {
                text.pos += delta;
            }
            CanvasObject::Shape(shape) => {
                shape.pos += delta;
            }
            CanvasObject::Stroke(stroke) => {
                // For strokes, move all points
                for point in &mut stroke.points {
                    *point += delta;
                }
            }
        }
    }

    /// Extracts transform information (position, size) from an object
    pub fn get_transform(&self) -> ObjectTransform {
        match self {
            CanvasObject::Image(img) => ObjectTransform {
                pos: img.pos,
                size: img.size,
            },
            CanvasObject::Text(text) => ObjectTransform {
                pos: text.pos,
                size: egui::vec2(text.font_size, text.font_size), // Using font_size for both dimensions
            },
            CanvasObject::Shape(shape) => ObjectTransform {
                pos: shape.pos,
                size: egui::vec2(shape.size, shape.size), // Using shape.size for both dimensions
            },
            CanvasObject::Stroke(stroke) => {
                let bbox = stroke.bounding_box();
                ObjectTransform {
                    pos: bbox.min,
                    size: bbox.size(),
                }
            }
        }
    }
}

impl CanvasObjectOps for CanvasObject {
    /// Delegates transform to the inner object type
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        drag_start: Pos2,
        current_pos: Pos2,
    ) {
        match self {
            CanvasObject::Image(img) => img.transform(handle, delta, drag_start, current_pos),
            CanvasObject::Text(text) => text.transform(handle, delta, drag_start, current_pos),
            CanvasObject::Shape(shape) => shape.transform(handle, delta, drag_start, current_pos),
            CanvasObject::Stroke(stroke) => {
                stroke.transform(handle, delta, drag_start, current_pos)
            }
        }
    }

    /// Delegates painting to the inner object type
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn paint(&self, painter: &egui::Painter, selected: bool, view_offset: egui::Vec2, zoom: f32) {
        match self {
            CanvasObject::Stroke(stroke) => stroke.paint(painter, selected, view_offset, zoom),
            CanvasObject::Image(image) => image.paint(painter, selected, view_offset, zoom),
            CanvasObject::Text(text) => text.paint(painter, selected, view_offset, zoom),
            CanvasObject::Shape(shape) => shape.paint(painter, selected, view_offset, zoom),
        }
    }

    /// Delegates bounding box calculation to the inner object type
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn bounding_box(&self) -> egui::Rect {
        match self {
            CanvasObject::Stroke(stroke) => stroke.bounding_box(),
            CanvasObject::Image(image) => image.bounding_box(),
            CanvasObject::Text(text) => text.bounding_box(),
            CanvasObject::Shape(shape) => shape.bounding_box(),
        }
    }
}

/// Window display mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WindowMode {
    Windowed,
    ExclusiveFullscreen,
    #[default]
    BorderlessFullscreen,
}

/// UI theme mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    System,
    Light,
    #[default]
    Dark,
}

/// GPU optimization policy for performance vs resource usage tradeoff
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OptimizationPolicy {
    #[default]
    Performance,
    ResourceUsage,
}

/// Graphics API backend selection
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GraphicsApi {
    #[cfg_attr(not(target_os = "windows"), default)]
    Auto,
    Vulkan,
    // on windows, using vulkan results in an 8-second hang after resizing the window, so we default to dx12 which is more stable
    #[cfg_attr(target_os = "windows", default)]
    Dx12,
    Metal,
    WebGpu,
    Gl,
}

impl GraphicsApi {
    pub fn to_backends(self) -> wgpu::Backends {
        match self {
            GraphicsApi::Auto => wgpu::Backends::all(),
            GraphicsApi::Vulkan => wgpu::Backends::VULKAN,
            GraphicsApi::Dx12 => wgpu::Backends::DX12,
            GraphicsApi::Metal => wgpu::Backends::METAL,
            GraphicsApi::WebGpu => wgpu::Backends::BROWSER_WEBGPU,
            GraphicsApi::Gl => wgpu::Backends::GL,
        }
    }
}

/// Represents the current state of the canvas including all objects
#[derive(Debug, Clone, Default)]
pub struct CanvasState {
    pub objects: Vec<CanvasObject>,
}

/// State for a single page including canvas and undo/redo history
#[derive(Debug, Clone)]
pub struct PageState {
    pub canvas: CanvasState,
    pub history: History,
    pub view_offset: egui::Vec2,
    pub view_zoom: f32,
}

impl Default for PageState {
    fn default() -> Self {
        Self {
            canvas: CanvasState::default(),
            history: History::default(),
            view_offset: egui::Vec2::ZERO,
            view_zoom: 1.0,
        }
    }
}

impl CanvasState {}

// 应用程序设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentState {
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default)]
    pub canvas_color: Color32,
    #[serde(default)]
    pub window_opacity: f32,

    #[serde(default)]
    pub stroke_smoothing: bool,
    #[serde(default)]
    pub stroke_straightening: bool,
    #[serde(default)]
    pub stroke_straightening_tolerance: f32,
    #[serde(default)]
    pub interpolation_frequency: f32,
    #[serde(default)]
    pub quick_colors: Vec<Color32>,

    #[serde(default)]
    pub show_fps: bool,
    #[serde(default)]
    pub window_mode: WindowMode,
    #[serde(default)]
    pub present_mode: PresentMode,
    #[serde(default)]
    pub optimization_policy: OptimizationPolicy,
    #[serde(default)]
    pub graphics_api: GraphicsApi,
    #[serde(default)]
    pub low_latency_mode: bool,
    #[serde(default)]
    pub force_redraw_every_frame: bool,

    #[serde(default)]
    pub keep_insertion_window_open: bool,

    #[serde(default)]
    pub show_welcome_window_on_start: bool,
    #[serde(default)]
    pub show_startup_animation: bool,

    #[serde(default)]
    pub easter_egg_redo: bool,
    #[serde(default)]
    pub click_or_drag_to_single_select: bool,
}

impl Default for PersistentState {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::default(),
            canvas_color: utils::get_default_canvas_color(),
            window_opacity: 1.0,

            stroke_smoothing: true,
            stroke_straightening: true,
            stroke_straightening_tolerance: 20.0,
            interpolation_frequency: 0.1,
            quick_colors: utils::get_default_quick_colors(),
            click_or_drag_to_single_select: false,

            show_fps: false,
            window_mode: WindowMode::default(),
            present_mode: PresentMode::AutoVsync,
            optimization_policy: OptimizationPolicy::default(),
            graphics_api: GraphicsApi::default(),
            low_latency_mode: false,
            force_redraw_every_frame: false,

            keep_insertion_window_open: true,

            show_welcome_window_on_start: true,
            show_startup_animation: true,

            easter_egg_redo: false,
        }
    }
}

impl PersistentState {
    // 获取设置文件路径
    fn get_settings_path() -> std::path::PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        path.push("uwu");
        std::fs::create_dir_all(&path).ok();
        path.push("settings.json");
        path
    }

    // 加载设置从文件
    pub fn load_from_file() -> Self {
        let settings_path = Self::get_settings_path();
        if let Ok(content) = std::fs::read_to_string(settings_path)
            && let Ok(settings) = serde_json::from_str(&content)
        {
            return settings;
        }
        Self::default()
    }

    // 保存设置到文件
    pub fn save_to_file(&self) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let settings_path = Self::get_settings_path();
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(settings_path, content)?;
        Ok(())
    }
}

// 绘图数据结构
#[derive(Debug, Clone)]
pub struct CanvasStroke {
    pub points: Vec<Pos2>,
    pub width: StrokeWidth,
    pub color: Color32,
    pub base_width: f32,
    pub shape: Option<CanvasShapeType>,
}

impl CanvasObjectOps for CanvasStroke {
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn transform(
        &mut self,
        handle: TransformHandle,
        _delta: egui::Vec2,
        _drag_start: Pos2,
        current_pos: Pos2,
    ) {
        let bbox = self.bounding_box();
        if bbox.width() < 1.0 || bbox.height() < 1.0 {
            return;
        }

        let (new_min, new_max) = match handle {
            TransformHandle::TopLeft => (current_pos, bbox.max),
            TransformHandle::Top => (Pos2::new(bbox.min.x, current_pos.y), bbox.max),
            TransformHandle::TopRight => (
                Pos2::new(bbox.min.x, current_pos.y),
                Pos2::new(current_pos.x, bbox.max.y),
            ),
            TransformHandle::Left => (Pos2::new(current_pos.x, bbox.min.y), bbox.max),
            TransformHandle::Right => (bbox.min, Pos2::new(current_pos.x, bbox.max.y)),
            TransformHandle::BottomLeft => (
                Pos2::new(current_pos.x, bbox.min.y),
                Pos2::new(bbox.max.x, current_pos.y),
            ),
            TransformHandle::Bottom => (bbox.min, Pos2::new(bbox.max.x, current_pos.y)),
            TransformHandle::BottomRight => (bbox.min, current_pos),
        };

        let new_width = (new_max.x - new_min.x).max(10.0);
        let new_height = (new_max.y - new_min.y).max(10.0);
        let scale_x = new_width / bbox.width();
        let scale_y = new_height / bbox.height();
        let avg_scale = (scale_x + scale_y) / 2.0;

        for point in &mut self.points {
            point.x = new_min.x + (point.x - bbox.min.x) * scale_x;
            point.y = new_min.y + (point.y - bbox.min.y) * scale_y;
        }

        match &mut self.width {
            StrokeWidth::Fixed(w) => *w = (*w * avg_scale).max(1.0),
            StrokeWidth::Dynamic(widths) => {
                for w in widths.iter_mut() {
                    *w = (*w * avg_scale).max(1.0);
                }
            }
        }
        self.base_width = (self.base_width * avg_scale).max(1.0);
    }

    #[cfg_attr(feature = "profiling", profiling::function)]
    fn bounding_box(&self) -> egui::Rect {
        if self.points.is_empty() {
            return egui::Rect::from_min_max(Pos2::ZERO, Pos2::ZERO);
        }

        // 计算所有点的最小和最大坐标
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for point in &self.points {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }

        // 考虑笔画宽度，添加一些边距
        let max_width = self.width.max_width();
        let padding = max_width / 2.0 + 5.0; // 添加额外的5像素边距

        egui::Rect::from_min_max(
            Pos2::new(min_x - padding, min_y - padding),
            Pos2::new(max_x + padding, max_y + padding),
        )
    }

    #[cfg_attr(feature = "profiling", profiling::function)]
    fn paint(&self, painter: &egui::Painter, selected: bool, view_offset: egui::Vec2, zoom: f32) {
        let color = if selected { Color32::BLUE } else { self.color };

        let first_point = (self.points[0] - view_offset) * zoom;
        let z_width_first = self.width.first() * zoom / 2.0;

        painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
            first_point,
            z_width_first,
            color,
        )));
        if self.points.len() >= 2 {
            let last_point = (self.points[self.points.len() - 1] - view_offset) * zoom;
            let z_width_last = self.width.last() * zoom / 2.0;
            painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                last_point,
                z_width_last,
                color,
            )));
            match &self.width {
                StrokeWidth::Fixed(w) => {
                    if self.points.len() == 2 {
                        let second_point = (self.points[1] - view_offset) * zoom;
                        painter.line_segment(
                            [first_point, second_point],
                            Stroke::new(*w * zoom, color),
                        );
                    } else {
                        let path_points: Vec<Pos2> = self
                            .points
                            .iter()
                            .map(|p| (*p - view_offset) * zoom)
                            .collect();
                        let path = egui::epaint::PathShape::line(
                            path_points,
                            Stroke::new(*w * zoom, color),
                        );
                        painter.add(egui::Shape::Path(path));
                    }
                }
                StrokeWidth::Dynamic(widths) => {
                    for (i, (p1, p2)) in self.points.iter().zip(self.points[1..].iter()).enumerate()
                    {
                        let p1 = (*p1 - view_offset) * zoom;
                        let p2 = (*p2 - view_offset) * zoom;
                        let avg_width = (widths[i] + widths[i + 1]) / 2.0 * zoom;
                        painter.line_segment([p1, p2], Stroke::new(avg_width, color));
                    }
                }
            }
        }

        if selected {
            let stroke_rect = self.bounding_box().translate(-view_offset);
            let stroke_rect =
                egui::Rect::from_min_size(stroke_rect.min * zoom, stroke_rect.size() * zoom);
            painter.rect_stroke(
                stroke_rect,
                0.0,
                Stroke::new(2.0_f32, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, stroke_rect);
        }
    }
}

// FPS 计数器
pub struct FpsCounter {
    pub frame_count: u32,
    pub last_time: Instant,
    pub current_fps: f32,
}

impl FpsCounter {
    pub fn new() -> Self {
        Self {
            frame_count: 0,
            last_time: Instant::now(),
            current_fps: 0.0,
        }
    }

    #[cfg_attr(feature = "profiling", profiling::function)]
    pub fn update(&mut self) -> f32 {
        self.frame_count += 1;

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_time).as_secs_f32();

        if elapsed >= 0.05 {
            self.current_fps = self.frame_count as f32 / elapsed;
            self.frame_count = 0;
            self.last_time = now;
        }

        self.current_fps
    }
}

// 单个正在绘制的笔画数据
pub struct ActiveStroke {
    pub points: Vec<Pos2>,
    pub width: StrokeWidth,
    pub times: Vec<f64>,             // 每个点的时间戳（用于速度计算）
    pub start_time: Instant,         // 笔画开始时间
    pub last_movement_time: Instant, // 最后一次移动的时间（用于检测停留）
}

/// Unified per-pointer interaction state for all tools
pub enum PointerInteraction {
    Drawing {
        active_stroke: ActiveStroke,
    },
    Selecting {
        drag_start: Pos2,
        dragged_handle: Option<TransformHandle>,
        drag_original_transforms: Vec<(usize, ObjectTransform)>,
        drag_accumulated_delta: egui::Vec2,
    },
    Erasing,
    ShapeInsert {
        start_pos: Pos2,
        shape_type: CanvasShapeType,
    },
    Panning {
        last_pos: Pos2,
    },
    MarqueeSelect {
        #[allow(dead_code)]
        drag_start: Pos2,
        points: Vec<Pos2>,
    },
}

/// Represents a single pointer (touch or mouse) on the canvas
pub struct PointerState {
    pub id: u64,
    pub pos: Pos2,
    pub prev_pos: Option<Pos2>,
    pub interaction: PointerInteraction,
}

/// State for multi-touch pinch-to-zoom gesture
#[derive(Debug, Clone)]
pub struct PinchState {
    pub initial_zoom: f32,
    pub initial_view_offset: egui::Vec2,
    pub initial_center_screen: Pos2,
    pub initial_distance: f32,
}

#[cfg(feature = "startup_animation")]
pub struct StartupAnimation {
    fps: f32,
    start_time: Option<Instant>,

    // Video
    frames: &'static [&'static [u8]],
    texture: Option<TextureHandle>,
    last_frame_index: usize,

    // Audio
    _audio_sink: Option<Player>,

    finished: bool,
}

#[cfg(feature = "startup_animation")]
impl StartupAnimation {
    pub fn new(fps: f32, frames: &'static [&'static [u8]], audio: &'static [u8]) -> Self {
        Self {
            fps,
            start_time: None,
            frames,
            texture: None,
            last_frame_index: usize::MAX,
            _audio_sink: Some(Self::play_audio(audio)),
            finished: false,
        }
    }

    fn play_audio(audio: &'static [u8]) -> Player {
        let handle = DeviceSinkBuilder::open_default_sink().expect("failed to open stream");

        let player = Player::connect_new(handle.mixer());

        let cursor = Cursor::new(audio);
        let source = Decoder::new(cursor).unwrap();

        handle.mixer().add(source);

        // keep stream alive
        std::mem::forget(handle);

        player
    }

    pub fn update(&mut self, ctx: &Context) {
        if self.finished {
            return;
        }

        let start = self.start_time.get_or_insert_with(Instant::now);
        let elapsed = start.elapsed().as_secs_f32();
        let frame_index = (elapsed * self.fps) as usize;

        if frame_index >= self.frames.len() {
            self.finished = true;
            return;
        }

        if frame_index == self.last_frame_index {
            return;
        }

        self.last_frame_index = frame_index;

        let image = image::load_from_memory(self.frames[frame_index])
            .expect("Invalid startup frame")
            .to_rgba8();

        let color_image = ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );

        match &mut self.texture {
            Some(tex) => tex.set(color_image, TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture(
                    "startup_animation",
                    color_image,
                    TextureOptions::LINEAR,
                ));
            }
        }
    }

    pub fn draw_fullscreen(&self, ctx: &Context) {
        if self.finished {
            return;
        }

        let Some(tex) = &self.texture else { return };

        let rect = ctx.content_rect();

        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("startup_animation"),
        ));

        painter.image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

// 历史记录命令枚举
#[derive(Debug, Clone)]
pub enum HistoryCommand {
    // 添加对象命令
    AddObject {
        index: usize,
        object: CanvasObject,
    },
    // 删除对象命令
    RemoveObject {
        index: usize,
        object: CanvasObject,
    },
    // 批量操作（用于清空画布等）
    ClearObjects {
        objects: Vec<CanvasObject>,
    },
    // 移动对象命令
    MoveObject {
        index: usize,
        old_position: egui::Vec2,
        new_position: egui::Vec2,
    },
    // 变换对象命令
    TransformObject {
        index: usize,
        old_transform: ObjectTransform,
        new_transform: ObjectTransform,
    },
    BatchCommand {
        commands: Vec<HistoryCommand>,
    },
}

// 对象变换信息
#[derive(Debug, Clone)]
pub struct ObjectTransform {
    pub pos: egui::Pos2,
    pub size: egui::Vec2,
}

// 历史记录结构
#[derive(Debug, Clone)]
pub struct History {
    pub undo_stack: Vec<HistoryCommand>,
    pub redo_stack: Vec<HistoryCommand>,
    pub max_history_size: usize,
}

impl History {
    pub fn new(max_history_size: usize) -> Self {
        Self {
            undo_stack: Vec::with_capacity(max_history_size),
            redo_stack: Vec::with_capacity(max_history_size),
            max_history_size,
        }
    }

    // 保存添加对象的命令
    pub fn save_add_object(&mut self, index: usize, object: CanvasObject) {
        let command = HistoryCommand::AddObject { index, object };
        self.push_command(command);
    }

    // 保存删除对象的命令
    pub fn save_remove_object(&mut self, index: usize, object: CanvasObject) {
        let command = HistoryCommand::RemoveObject { index, object };
        self.push_command(command);
    }

    // 保存清空对象的命令
    pub fn save_clear_objects(&mut self, objects: Vec<CanvasObject>) {
        let command = HistoryCommand::ClearObjects { objects };
        self.push_command(command);
    }

    // 保存移动对象的命令
    pub fn save_move_object(
        &mut self,
        index: usize,
        old_position: egui::Vec2,
        new_position: egui::Vec2,
    ) {
        let command = HistoryCommand::MoveObject {
            index,
            old_position,
            new_position,
        };
        self.push_command(command);
    }

    // 保存变换对象的命令
    pub fn save_transform_object(
        &mut self,
        index: usize,
        old_transform: ObjectTransform,
        new_transform: ObjectTransform,
    ) {
        let command = HistoryCommand::TransformObject {
            index,
            old_transform,
            new_transform,
        };
        self.push_command(command);
    }

    // 保存批量操作命令
    pub fn save_batch(&mut self, commands: Vec<HistoryCommand>) {
        self.push_command(HistoryCommand::BatchCommand { commands });
    }

    // 推送命令并维护历史记录大小
    pub(crate) fn push_command(&mut self, command: HistoryCommand) {
        self.undo_stack.push(command);
        self.redo_stack.clear();

        // 清理超出限制的历史记录
        if self.undo_stack.len() > self.max_history_size {
            self.undo_stack.remove(0);
        }
    }

    // 执行撤销操作
    pub fn undo(&mut self, current_state: &mut CanvasState) -> bool {
        if let Some(command) = self.undo_stack.pop() {
            self.apply_reverse(&command, current_state);
            self.redo_stack.push(command);
            true
        } else {
            false
        }
    }

    // 执行重做操作
    pub fn redo(&mut self, current_state: &mut CanvasState) -> bool {
        if let Some(command) = self.redo_stack.pop() {
            self.apply_forward(&command, current_state);
            self.undo_stack.push(command);
            true
        } else {
            false
        }
    }

    fn apply_reverse(&self, command: &HistoryCommand, current_state: &mut CanvasState) {
        match command {
            HistoryCommand::AddObject { index, object: _ } => {
                if *index < current_state.objects.len() {
                    current_state.objects.remove(*index);
                }
            }
            HistoryCommand::RemoveObject { index, object } => {
                if *index <= current_state.objects.len() {
                    current_state.objects.insert(*index, object.clone());
                }
            }
            HistoryCommand::ClearObjects { objects } => {
                current_state.objects = objects.clone();
            }
            HistoryCommand::MoveObject {
                index,
                old_position,
                new_position: _,
            } => {
                if *index < current_state.objects.len() {
                    CanvasObject::move_object(&mut current_state.objects[*index], *old_position);
                }
            }
            HistoryCommand::TransformObject {
                index,
                old_transform,
                new_transform: _,
            } => {
                if *index < current_state.objects.len() {
                    History::apply_transform(&mut current_state.objects[*index], old_transform);
                }
            }
            HistoryCommand::BatchCommand { commands } => {
                for cmd in commands.iter().rev() {
                    self.apply_reverse(cmd, current_state);
                }
            }
        }
    }

    fn apply_forward(&self, command: &HistoryCommand, current_state: &mut CanvasState) {
        match command {
            HistoryCommand::AddObject { index, object } => {
                if *index <= current_state.objects.len() {
                    current_state.objects.insert(*index, object.clone());
                }
            }
            HistoryCommand::RemoveObject { index, object: _ } => {
                if *index < current_state.objects.len() {
                    current_state.objects.remove(*index);
                }
            }
            HistoryCommand::ClearObjects { objects: _ } => {
                current_state.objects.clear();
            }
            HistoryCommand::MoveObject {
                index,
                old_position: _,
                new_position,
            } => {
                if *index < current_state.objects.len() {
                    CanvasObject::move_object(&mut current_state.objects[*index], *new_position);
                }
            }
            HistoryCommand::TransformObject {
                index,
                old_transform: _,
                new_transform,
            } => {
                if *index < current_state.objects.len() {
                    History::apply_transform(&mut current_state.objects[*index], new_transform);
                }
            }
            HistoryCommand::BatchCommand { commands } => {
                for cmd in commands.iter() {
                    self.apply_forward(cmd, current_state);
                }
            }
        }
    }

    fn apply_transform(object: &mut CanvasObject, transform: &ObjectTransform) {
        match object {
            CanvasObject::Image(img) => {
                img.pos = transform.pos;
                img.size = transform.size;
            }
            CanvasObject::Text(text) => {
                text.pos = transform.pos;
                text.font_size = transform.size.x;
                text.cached_size = None;
            }
            CanvasObject::Shape(shape) => {
                shape.pos = transform.pos;
                shape.size = transform.size.x;
            }
            CanvasObject::Stroke(stroke) => {
                let old_bbox = stroke.bounding_box();
                if old_bbox.width() < 1.0 || old_bbox.height() < 1.0 {
                    return;
                }
                let scale_x = transform.size.x / old_bbox.width();
                let scale_y = transform.size.y / old_bbox.height();
                let avg_scale = (scale_x + scale_y) / 2.0;
                for point in &mut stroke.points {
                    point.x = transform.pos.x + (point.x - old_bbox.min.x) * scale_x;
                    point.y = transform.pos.y + (point.y - old_bbox.min.y) * scale_y;
                }
                match &mut stroke.width {
                    StrokeWidth::Fixed(w) => *w = (*w * avg_scale).max(1.0),
                    StrokeWidth::Dynamic(widths) => {
                        for w in widths.iter_mut() {
                            *w = (*w * avg_scale).max(1.0);
                        }
                    }
                }
                stroke.base_width = (stroke.base_width * avg_scale).max(1.0);
            }
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(50)
    }
}

/// Marquee multi-select matching mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarqueeMatchMode {
    #[default]
    /// Select objects whose bounding box intersects the marquee rect
    Overlapping,
    /// Select objects whose bounding box is fully inside the marquee rect
    Containing,
}

/// Deferred commands that execute at the start of the next redraw frame.
#[derive(Debug, Clone, Copy)]
pub enum AppCommand {
    SetPresentMode(wgpu::PresentMode),
    UpdateCursorHittest,
}

// 应用程序状态
pub struct AppState {
    // canvas states
    pub canvas: CanvasState,                             // 当前页面的画布
    pub history: History,                                // 当前页面的历史记录
    pub pages: Vec<PageState>,                           // 分页
    pub current_page: usize,                             // 当前页码
    pub pointers: HashMap<u64, PointerState>, // 统一指针状态表 (鼠标 id=0, 触控使用 winit touch id)
    pub brush_color: Color32,                 // 画笔颜色
    pub brush_width: f32,                     // 画笔大小
    pub dynamic_brush_width_mode: DynamicBrushWidthMode, // 动态画笔大小微调
    pub view_offset: egui::Vec2,              // 画布视图偏移
    pub view_zoom: f32,                       // 画布视图缩放 (1.0 = 100%)
    pub pinch_state: Option<PinchState>,      // 双指缩放状态
    pub current_tool: CanvasTool,             // 当前工具
    pub current_insert_tab: InsertTab,        // 插入工具的当前标签页
    pub selected_shape_type: Option<CanvasShapeType>, // 插入形状时选中的形状类型
    pub continuous_insert: bool,              // 是否连续插入形状
    pub shapes_inserted_count: u32,           // 已插入形状的计数
    pub eraser_size: f32,                     // 橡皮擦大小
    pub marquee_match_mode: MarqueeMatchMode, // 多选框匹配模式
    pub selected_object_indices: Vec<usize>,  // 选中的对象索引

    // persistent states
    pub persistent: PersistentState,

    // ui states
    pub show_quick_color_edit_window: bool, // 是否显示快捷颜色编辑器
    pub show_welcome_window: bool,
    pub show_page_management_window: bool,
    pub toolbar_expanded: bool,

    pub show_size_preview: bool,
    pub new_text_content: String,
    pub should_quit: bool,
    pub fullscreen_video_modes: Vec<winit::monitor::VideoModeHandle>,
    pub selected_video_mode_index: Option<usize>, // 选中的视频模式索引
    pub fps_counter: FpsCounter,                  // FPS 计数器
    pub new_quick_color: Color32,                 // 新快捷颜色，用于添加
    pub show_touch_points: bool,                  // 是否显示触控点，用于调试

    pub is_overlay_mode: bool,

    // screenshot states
    pub screenshot_path: Option<PathBuf>,

    // cached states
    pub active_backend: Option<Backend>,
    pub cursor_position: PhysicalPosition<f64>,

    // deferred commands
    pub command_queue: Vec<AppCommand>,

    #[cfg(feature = "startup_animation")]
    pub startup_animation: Option<StartupAnimation>, // 启动动画

    // utils
    pub toasts: Toasts,
}

impl Default for AppState {
    fn default() -> Self {
        let default_page = PageState::default();
        Self {
            canvas: default_page.canvas.clone(),
            pages: vec![default_page.clone()],
            current_page: 0,
            pointers: HashMap::new(),
            brush_color: Color32::WHITE,
            brush_width: 3.0,
            dynamic_brush_width_mode: DynamicBrushWidthMode::default(),
            view_offset: default_page.view_offset,
            view_zoom: default_page.view_zoom,
            pinch_state: None,
            current_tool: CanvasTool::Brush,
            current_insert_tab: InsertTab::Shape,
            selected_shape_type: None,
            continuous_insert: false,
            shapes_inserted_count: 0,
            eraser_size: 10.0,
            marquee_match_mode: MarqueeMatchMode::default(),
            selected_object_indices: Vec::new(),
            show_size_preview: false,
            fps_counter: FpsCounter::new(),
            should_quit: false,
            new_text_content: "".to_string(),
            fullscreen_video_modes: Vec::new(),
            selected_video_mode_index: None,
            show_quick_color_edit_window: false,
            new_quick_color: Color32::WHITE,
            show_touch_points: false,
            show_welcome_window: true,
            show_page_management_window: false,
            toolbar_expanded: false,
            persistent: PersistentState::load_from_file(),
            screenshot_path: None,
            toasts: Toasts::default()
                .with_anchor(egui_notify::Anchor::BottomRight)
                .with_margin(egui::vec2(20.0, 20.0)),
            history: History::default(),
            active_backend: None,
            command_queue: Vec::with_capacity(3),
            is_overlay_mode: false,
            cursor_position: PhysicalPosition {
                x: 0.0_f64,
                y: 0.0_f64,
            },
            #[cfg(feature = "startup_animation")]
            startup_animation: None,
        }
    }
}

impl AppState {
    pub const MIN_ZOOM: f32 = 0.1;
    pub const MAX_ZOOM: f32 = 10.0;
    pub const ZOOM_STEP: f32 = 0.03;

    /// Initialize pinch-to-zoom state if there are at least two panning pointers
    pub fn init_pinch_if_two_panning(&mut self) {
        let panning: Vec<(u64, Pos2)> = self
            .pointers
            .iter()
            .filter_map(|(id, p)| {
                if let PointerInteraction::Panning { last_pos } = &p.interaction {
                    Some((*id, *last_pos))
                } else {
                    None
                }
            })
            .collect();

        if panning.len() < 2 {
            return;
        }

        let (_, s0) = panning[0];
        let (_, s1) = panning[1];
        let c = Pos2::new((s0.x + s1.x) / 2.0, (s0.y + s1.y) / 2.0);
        let d = s0.distance(s1);

        if d < 1.0 {
            return;
        }

        self.pinch_state = Some(PinchState {
            initial_zoom: self.view_zoom,
            initial_view_offset: self.view_offset,
            initial_center_screen: c,
            initial_distance: d,
        });
    }

    /// Apply pinch-to-zoom transformation using current pointer positions
    pub fn apply_pinch_zoom(&mut self) {
        let panning: Vec<(u64, Pos2)> = self
            .pointers
            .iter()
            .filter_map(|(id, p)| {
                if let PointerInteraction::Panning { last_pos } = &p.interaction {
                    Some((*id, *last_pos))
                } else {
                    None
                }
            })
            .collect();

        if panning.len() < 2 {
            self.pinch_state = None;
            return;
        }

        let pinch = match &self.pinch_state {
            Some(p) => p.clone(),
            None => return,
        };

        let (_, s0) = panning[0];
        let (_, s1) = panning[1];
        let c_new = Pos2::new((s0.x + s1.x) / 2.0, (s0.y + s1.y) / 2.0);
        let d_new = s0.distance(s1);

        if d_new < 1.0 {
            return;
        }

        let z_new = (pinch.initial_zoom * d_new / pinch.initial_distance)
            .clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        self.view_zoom = z_new;
        self.view_offset = pinch.initial_view_offset
            + pinch.initial_center_screen.to_vec2() / pinch.initial_zoom
            - c_new.to_vec2() / z_new;
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected_object_indices.contains(&index)
    }

    pub fn clear_selection(&mut self) {
        self.selected_object_indices.clear();
    }

    pub fn toggle_selection(&mut self, index: usize) {
        if self.is_selected(index) {
            self.selected_object_indices.retain(|&i| i != index);
        } else {
            self.selected_object_indices.push(index);
        }
    }
}
