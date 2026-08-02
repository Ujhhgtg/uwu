pub mod flat;

use egui::{Color32, Pos2, Stroke};
use egui_notify::Toasts;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::Backend;
use wgpu::PresentMode;
use winit::dpi::PhysicalPosition;

use crate::utils;
use crate::utils::plugins::LoadedPlugin;

/// Magic header for canvas files: `b"UWU"` followed by format version byte.
/// Must be kept in sync with [`CANVAS_FILE_HEADER`].
pub(crate) const CANVAS_FILE_MAGIC: &[u8; 3] = b"UWU";
pub(crate) const CANVAS_FILE_VERSION: u8 = 5;
pub(crate) const CANVAS_FILE_EXT: &str = "owo"; // open whiteboard objects

pub(crate) fn make_canvas_file_header() -> [u8; 4] {
    let mut h = [0u8; 4];
    h[..3].copy_from_slice(CANVAS_FILE_MAGIC);
    h[3] = CANVAS_FILE_VERSION;
    h
}

/// Window inside which a second exit request confirms quitting.
pub const EXIT_CONFIRM_TIMEOUT: Duration = Duration::from_secs(3);

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

    #[allow(unused)]
    pub fn len(&self) -> Option<usize> {
        match self {
            StrokeWidth::Fixed(_) => None,
            StrokeWidth::Dynamic(v) => Some(v.len()),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            StrokeWidth::Fixed(_) => false,
            StrokeWidth::Dynamic(v) => v.is_empty(),
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
    Passthrough, // Only available in overlay mode; passes clicks through to underlying windows
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
    pub image_data: Arc<[u8]>, // RGBA pixel data for export
    pub image_size: [u32; 2],  // [width, height] of the original image
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
        let old_size = bbox.size().max(egui::vec2(1.0, 1.0));
        let aspect_ratio = self.aspect_ratio.max(0.01); // width / height
        const MIN_SIZE: f32 = 10.0;

        // Desired size from the raw drag, before aspect correction.
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
        let raw_width = (new_max.x - new_min.x).max(MIN_SIZE);
        let raw_height = (new_max.y - new_min.y).max(MIN_SIZE);

        // Aspect-preserving size, driven by the dominant axis of the drag.
        let (width, height) = match handle {
            TransformHandle::Left | TransformHandle::Right => {
                let width = raw_width;
                (width, (width / aspect_ratio).max(MIN_SIZE))
            }
            TransformHandle::Top | TransformHandle::Bottom => {
                let height = raw_height;
                ((height * aspect_ratio).max(MIN_SIZE), height)
            }
            _ => {
                let scale = (raw_width / old_size.x).max(raw_height / old_size.y);
                let width = (old_size.x * scale).max(MIN_SIZE);
                let height = (width / aspect_ratio).max(MIN_SIZE);
                (width, height)
            }
        };

        self.size = egui::vec2(width, height);
        self.pos = match handle {
            // Anchor on the edge/corner opposite the dragged handle.
            TransformHandle::TopLeft => Pos2::new(bbox.max.x - width, bbox.max.y - height),
            TransformHandle::Top => Pos2::new(bbox.center().x - width / 2.0, bbox.max.y - height),
            TransformHandle::TopRight => Pos2::new(bbox.min.x, bbox.max.y - height),
            TransformHandle::Left => Pos2::new(bbox.max.x - width, bbox.center().y - height / 2.0),
            TransformHandle::Right => Pos2::new(bbox.min.x, bbox.center().y - height / 2.0),
            TransformHandle::BottomLeft => Pos2::new(bbox.max.x - width, bbox.min.y),
            TransformHandle::Bottom => Pos2::new(bbox.center().x - width / 2.0, bbox.min.y),
            TransformHandle::BottomRight => Pos2::new(bbox.min.x, bbox.min.y),
        };
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
    /// Real laid-out size in canvas coordinates, cached by `paint`.
    pub cached_size: Cell<Option<egui::Vec2>>,
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
                self.cached_size.set(None);
            }
        }
    }

    /// Returns the bounding rectangle for the text
    #[cfg_attr(feature = "profiling", profiling::function)]
    fn bounding_box(&self) -> egui::Rect {
        if let Some(size) = self.cached_size.get() {
            egui::Rect::from_min_size(self.pos, size)
        } else {
            // Fallback until the first paint caches the real galley size.
            // CJK glyphs are full-width; Latin glyphs roughly 0.6em.
            // (text.len() counts bytes, which would overestimate CJK ~3x.)
            let approx_width: f32 = self
                .text
                .chars()
                .map(|c| {
                    if (c as u32) >= 0x2E80 {
                        self.font_size
                    } else {
                        self.font_size * 0.6
                    }
                })
                .sum();
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
        // Cache the unzoomed size so hit-testing and selection use the real
        // text bounds instead of a byte-count approximation.
        self.cached_size.set(Some(text_galley.size() / zoom));
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
    pub show_welcome_window_on_start: bool,

    /// 插入后保持插入窗口（合并自原「连续插入」），持久化，默认关闭。
    #[serde(default)]
    pub continuous_insert: bool,

    #[serde(default)]
    pub easter_egg_redo: bool,
    #[serde(default)]
    pub click_or_drag_to_single_select: bool,

    #[serde(default)]
    pub disable_edge_gestures: bool,

    #[serde(default)]
    pub plugin_paths: Vec<PathBuf>,
}

impl Default for PersistentState {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::default(),
            canvas_color: utils::get_default_canvas_color(),

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

            show_welcome_window_on_start: true,
            continuous_insert: false,

            easter_egg_redo: false,
            disable_edge_gestures: false,
            plugin_paths: Vec::new(),
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
        // Write to a temporary file in the same directory and rename it, so a
        // crash mid-write cannot leave a truncated/corrupt settings file.
        let tmp_path = settings_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, content)?;
        if let Err(err) = std::fs::rename(&tmp_path, &settings_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(err.into());
        }
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

impl Default for FpsCounter {
    fn default() -> Self {
        Self::new()
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
    Erasing {
        /// Snapshot of the full object list taken when the eraser gesture started.
        original_objects: Vec<CanvasObject>,
        /// Whether any stroke was actually modified or removed during this gesture.
        modified: bool,
    },
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
    // Replaces the whole object list (used by pixel eraser gestures).
    ReplaceObjects {
        old: Vec<CanvasObject>,
        new: Vec<CanvasObject>,
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
    #[allow(unused)]
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
        if !commands.is_empty() {
            self.push_command(HistoryCommand::BatchCommand { commands });
        }
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
            HistoryCommand::ReplaceObjects { old, new: _ } => {
                current_state.objects = old.clone();
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
            HistoryCommand::ReplaceObjects { old: _, new } => {
                current_state.objects = new.clone();
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
                text.cached_size.set(None);
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
#[derive(Debug, Clone)]
pub enum AppCommand {
    SetPresentMode(wgpu::PresentMode),
    UpdateCursorHittest,
    LoadPlugin(std::path::PathBuf),
    // FIXME: exiting after doing this triggers a SIGSEGV on linux
    // UnloadAllPlugins,
}

// 应用程序状态
pub struct AppState {
    // canvas states
    pub canvas: CanvasState,                             // 当前页面的画布
    pub history: History,                                // 当前页面的历史记录
    pub pages: Vec<PageState>,                           // 分页
    pub current_page: usize,                             // 当前页码
    pub pointers: HashMap<u64, PointerState>, // 统一指针状态表 (鼠标 id=0, 触控使用 winit touch id)
    pub egui_window_rects: Vec<egui::Rect>,   // 保存所有 egui 窗口的 Rect，用于触控 Hittest
    pub brush_color: Color32,                 // 画笔颜色
    pub brush_width: f32,                     // 画笔大小
    pub dynamic_brush_width_mode: DynamicBrushWidthMode, // 动态画笔大小微调
    pub view_offset: egui::Vec2,              // 画布视图偏移
    pub view_zoom: f32,                       // 画布视图缩放 (1.0 = 100%)
    pub pinch_state: Option<PinchState>,      // 双指缩放状态
    pub current_tool: CanvasTool,             // 当前工具
    pub current_insert_tab: InsertTab,        // 插入工具的当前标签页
    pub selected_shape_type: Option<CanvasShapeType>, // 插入形状时选中的形状类型
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
    /// First generic exit request (Esc / window close) timestamp; a second
    /// request inside [`EXIT_CONFIRM_TIMEOUT`] confirms the exit.
    pub exit_confirm_armed_at: Option<Instant>,
    /// Toolbar "退出" button entered its confirm state at this time.
    pub toolbar_exit_confirm_at: Option<Instant>,
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

    // utils
    pub toasts: Toasts,

    /// Loaded plugins.
    pub plugins: Vec<LoadedPlugin>,

    pub initial_file: Option<PathBuf>,

    /// egui context of the toolbar helper window, so theme and canvas color
    /// changes can be mirrored to the other window in overlay mode.
    pub auxiliary_ctx: Option<egui::Context>,
}

impl Default for AppState {
    fn default() -> Self {
        let default_page = PageState::default();
        Self {
            canvas: default_page.canvas.clone(),
            pages: vec![default_page.clone()],
            current_page: 0,
            pointers: HashMap::new(),
            egui_window_rects: Vec::new(),
            brush_color: Color32::WHITE,
            brush_width: 3.0,
            dynamic_brush_width_mode: DynamicBrushWidthMode::default(),
            view_offset: default_page.view_offset,
            view_zoom: default_page.view_zoom,
            pinch_state: None,
            current_tool: CanvasTool::Brush,
            current_insert_tab: InsertTab::Shape,
            selected_shape_type: None,
            shapes_inserted_count: 0,
            eraser_size: 10.0,
            marquee_match_mode: MarqueeMatchMode::default(),
            selected_object_indices: Vec::new(),
            show_size_preview: false,
            fps_counter: FpsCounter::new(),
            should_quit: false,
            new_text_content: "".to_string(),
            fullscreen_video_modes: Vec::new(),
            exit_confirm_armed_at: None,
            toolbar_exit_confirm_at: None,
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
            plugins: Vec::new(),
            history: History::default(),
            active_backend: None,
            command_queue: Vec::with_capacity(3),
            is_overlay_mode: false,
            cursor_position: PhysicalPosition {
                x: 0.0_f64,
                y: 0.0_f64,
            },
            initial_file: None,
            auxiliary_ctx: None,
        }
    }
}

impl AppState {
    pub const MIN_ZOOM: f32 = 0.1;
    pub const MAX_ZOOM: f32 = 10.0;
    pub const ZOOM_STEP: f32 = 0.03;

    /// Arms or confirms a generic exit request (Esc / window close).
    /// Returns `true` when this is the second request inside the timeout
    /// window, meaning the caller should exit; otherwise shows a toast.
    pub fn request_exit(&mut self, toast: &str) -> bool {
        if self
            .exit_confirm_armed_at
            .is_some_and(|t| t.elapsed() < EXIT_CONFIRM_TIMEOUT)
        {
            self.exit_confirm_armed_at = None;
            true
        } else {
            self.exit_confirm_armed_at = Some(Instant::now());
            self.toasts.info(toast);
            false
        }
    }

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

    /// Ends a pixel-eraser gesture: removes the pointer and records a single
    /// history command covering every change made since the gesture started.
    pub fn finish_pixel_erasing(&mut self, pointer_id: u64) {
        let Some(pointer) = self.pointers.remove(&pointer_id) else {
            return;
        };
        let PointerInteraction::Erasing {
            original_objects,
            modified,
        } = pointer.interaction
        else {
            return;
        };
        if modified {
            let new_objects = std::mem::take(&mut self.canvas.objects);
            self.history.push_command(HistoryCommand::ReplaceObjects {
                old: original_objects,
                new: new_objects.clone(),
            });
            self.canvas.objects = new_objects;
        }
    }

    pub fn toggle_selection(&mut self, index: usize) {
        if self.is_selected(index) {
            self.selected_object_indices.retain(|&i| i != index);
        } else {
            self.selected_object_indices.push(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_dummy_object() -> CanvasObject {
        CanvasObject::Shape(CanvasShape {
            shape_type: CanvasShapeType::Circle,
            pos: Pos2::new(0.0, 0.0),
            size: 10.0,
            color: Color32::WHITE,
        })
    }

    #[test]
    fn test_history_undo_redo() {
        let mut history = History::new(10);
        let mut state = CanvasState::default();

        let obj = create_dummy_object();

        // Add object
        state.objects.push(obj.clone());
        history.save_add_object(0, obj.clone());

        assert_eq!(state.objects.len(), 1);
        assert_eq!(history.undo_stack.len(), 1);
        assert_eq!(history.redo_stack.len(), 0);

        // Undo
        let undo_success = history.undo(&mut state);
        assert!(undo_success);
        assert_eq!(state.objects.len(), 0);
        assert_eq!(history.undo_stack.len(), 0);
        assert_eq!(history.redo_stack.len(), 1);

        // Redo
        let redo_success = history.redo(&mut state);
        assert!(redo_success);
        assert_eq!(state.objects.len(), 1);
        assert_eq!(history.undo_stack.len(), 1);
        assert_eq!(history.redo_stack.len(), 0);
    }

    #[test]
    fn test_history_limit() {
        let mut history = History::new(2);

        for i in 0..5 {
            history.save_add_object(i, create_dummy_object());
        }

        assert_eq!(history.undo_stack.len(), 2);
    }

    /// Mirrors the command sequence the UI pushes for "置顶" (bring to front):
    /// RemoveObject for each removed index (in removal order), then AddObject
    /// for each re-appended object (in insertion order). Undo must restore the
    /// original list instead of dropping the moved objects.
    #[test]
    fn test_bring_to_front_batch_undo_redo() {
        let objs = ["A", "B", "C", "D"]
            .into_iter()
            .map(|_| create_dummy_object())
            .collect::<Vec<_>>();
        let obj_debug =
            |list: &[CanvasObject]| list.iter().map(|o| format!("{o:?}")).collect::<Vec<_>>();
        let mut state = CanvasState {
            objects: objs.clone(),
        };
        let mut history = History::new(10);

        // Select A (0) and B (1), move them to the end.
        let mut indices = [0usize, 1];
        indices.sort_unstable();
        let mut moved = Vec::new();
        let mut commands = Vec::new();
        for &idx in indices.iter().rev() {
            let obj = state.objects.remove(idx);
            commands.push(HistoryCommand::RemoveObject {
                index: idx,
                object: obj.clone(),
            });
            moved.push(obj);
        }
        moved.reverse();
        for obj in moved {
            let new_idx = state.objects.len();
            state.objects.push(obj.clone());
            commands.push(HistoryCommand::AddObject {
                index: new_idx,
                object: obj,
            });
        }
        history.save_batch(commands);

        // Undo: the original object list must be fully restored.
        assert!(history.undo(&mut state));
        assert_eq!(state.objects.len(), objs.len());
        assert_eq!(obj_debug(&state.objects), obj_debug(&objs));

        // Redo: front two objects are appended at the end again.
        assert!(history.redo(&mut state));
        assert_eq!(state.objects.len(), objs.len());
        assert_eq!(obj_debug(&state.objects[2..3]), obj_debug(&objs[0..1]));
        assert_eq!(obj_debug(&state.objects[3..4]), obj_debug(&objs[1..2]));
    }

    /// Mirrors the command sequence for "置底" (bring to back).
    #[test]
    fn test_bring_to_back_batch_undo_redo() {
        let objs = ["A", "B", "C", "D"]
            .into_iter()
            .map(|_| create_dummy_object())
            .collect::<Vec<_>>();
        let obj_debug =
            |list: &[CanvasObject]| list.iter().map(|o| format!("{o:?}")).collect::<Vec<_>>();
        let mut state = CanvasState {
            objects: objs.clone(),
        };
        let mut history = History::new(10);

        // Select C (2) and D (3), move them to the beginning.
        let mut indices = [2usize, 3];
        indices.sort_unstable();
        let mut moved = Vec::new();
        let mut commands = Vec::new();
        for &idx in indices.iter().rev() {
            let obj = state.objects.remove(idx);
            commands.push(HistoryCommand::RemoveObject {
                index: idx,
                object: obj.clone(),
            });
            moved.push((idx, obj));
        }
        moved.reverse();
        for (_, obj) in &moved {
            state.objects.insert(0, obj.clone());
            commands.push(HistoryCommand::AddObject {
                index: 0,
                object: obj.clone(),
            });
        }
        history.save_batch(commands);

        assert_eq!(state.objects.len(), objs.len());
        assert_eq!(obj_debug(&state.objects[0..1]), obj_debug(&objs[2..3]));
        assert_eq!(obj_debug(&state.objects[1..2]), obj_debug(&objs[3..4]));

        assert!(history.undo(&mut state));
        assert_eq!(obj_debug(&state.objects), obj_debug(&objs));

        assert!(history.redo(&mut state));
        assert_eq!(obj_debug(&state.objects[0..1]), obj_debug(&objs[2..3]));
        assert_eq!(obj_debug(&state.objects[1..2]), obj_debug(&objs[3..4]));
    }

    #[test]
    fn test_pixel_erasing_finish_records_single_replace_command() {
        let mut state = AppState::default();
        let original = CanvasState {
            objects: vec![create_dummy_object(), create_dummy_object()],
        };
        state.canvas = original.clone();
        state.pointers.insert(
            42,
            PointerState {
                id: 42,
                pos: Pos2::ZERO,
                prev_pos: None,
                interaction: PointerInteraction::Erasing {
                    original_objects: original.objects.clone(),
                    modified: true,
                },
            },
        );

        // Simulate the eraser shrinking the object list during the gesture.
        state.canvas.objects.pop();

        state.finish_pixel_erasing(42);

        // Exactly one command, covering the whole gesture.
        assert_eq!(state.history.undo_stack.len(), 1);
        assert!(state.pointers.is_empty());

        // Undo restores the pre-gesture snapshot; redo reapplies the result.
        assert!(state.history.undo(&mut state.canvas));
        assert_eq!(state.canvas.objects.len(), original.objects.len());
        assert!(state.history.redo(&mut state.canvas));
        assert_eq!(state.canvas.objects.len(), original.objects.len() - 1);
    }

    #[test]
    fn test_image_transform_preserves_aspect_ratio() {
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "aspect_test",
            egui::ColorImage::from_rgba_unmultiplied([2, 1], &[255; 8]),
            egui::TextureOptions::LINEAR,
        );
        let mut image = CanvasImage {
            texture,
            pos: Pos2::new(0.0, 0.0),
            size: egui::vec2(100.0, 50.0),
            aspect_ratio: 2.0,
            image_data: Arc::from(vec![255u8; 8].as_slice()),
            image_size: [2, 1],
        };

        // Corner drag: anchored at the opposite corner, aspect kept.
        image.transform(
            TransformHandle::BottomRight,
            egui::Vec2::ZERO,
            Pos2::ZERO,
            Pos2::new(200.0, 100.0),
        );
        let size = image.bounding_box().size();
        assert!((size.x / size.y - 2.0).abs() < 0.01);
        assert_eq!(image.pos, Pos2::new(0.0, 0.0));

        // Edge drag: the other dimension follows to keep the aspect ratio.
        image.transform(
            TransformHandle::Right,
            egui::Vec2::ZERO,
            Pos2::ZERO,
            Pos2::new(300.0, 50.0),
        );
        let size = image.bounding_box().size();
        assert!((size.x / size.y - 2.0).abs() < 0.01);
        assert_eq!(image.pos.x, 0.0);
    }

    #[test]
    fn test_text_bounding_box_uses_char_count_not_byte_count() {
        let text = CanvasText {
            text: "你好".to_string(),
            pos: Pos2::new(0.0, 0.0),
            color: Color32::WHITE,
            font_size: 20.0,
            cached_size: std::cell::Cell::new(None),
        };
        let bbox = text.bounding_box();
        // Two CJK chars ≈ 2 * font_size wide. The old byte-count fallback
        // would have reported ~6 * 0.6 * font_size = 72 px.
        assert!((bbox.width() - 40.0).abs() < 1.0);
    }

    #[test]
    fn test_request_exit_requires_second_request_within_timeout() {
        let mut state = AppState::default();

        // First request only arms the confirmation.
        assert!(!state.request_exit("再次按 Esc 确认退出"));
        assert!(state.exit_confirm_armed_at.is_some());

        // A second request inside the timeout window confirms the exit.
        assert!(state.request_exit("再次按 Esc 确认退出"));
        assert!(state.exit_confirm_armed_at.is_none());
    }
}
