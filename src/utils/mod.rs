pub mod dark_mode;
pub mod single_instance;
pub mod stroke;
pub mod ui;

#[cfg(windows)]
#[allow(non_snake_case)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

use egui::{Color32, Painter, Pos2, Rect, Stroke};
use image::{DynamicImage, GenericImageView};
use ttf_parser::{Face, OutlineBuilder};

use crate::state::{CanvasStroke, DynamicBrushWidthMode, StrokeWidth, TransformHandle};

// 检查点是否与笔画相交（用于对象橡皮擦）
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn point_intersects_stroke(pos: Pos2, stroke: &CanvasStroke, eraser_size: f32) -> bool {
    let eraser_radius = eraser_size / 2.0;
    if stroke.points.len() == 1 {
        let dist = pos.distance(stroke.points[0]);
        return dist <= eraser_radius + stroke.width.first() / 2.0;
    }
    for i in 0..stroke.points.len() - 1 {
        let p1 = stroke.points[i];
        let p2 = stroke.points[i + 1];
        let w1 = stroke.width.get(i);
        let w2 = stroke.width.get(i + 1);
        let stroke_width = w1.max(w2);

        // 计算点到线段的距离
        let dist = point_to_line_segment_distance(pos, p1, p2);
        if dist <= eraser_radius + stroke_width / 2.0 {
            return true;
        }
    }
    false
}

// 计算点到线段的最短距离
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn point_to_line_segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = Pos2::new(b.x - a.x, b.y - a.y);
    let ap = Pos2::new(p.x - a.x, p.y - a.y);
    let ab_sq = ab.x * ab.x + ab.y * ab.y;

    if ab_sq < 0.0001 {
        // a 和 b 几乎重合
        return (p.x - a.x).hypot(p.y - a.y);
    }

    let t = ((ap.x * ab.x + ap.y * ab.y) / ab_sq).clamp(0.0, 1.0);
    let closest = Pos2::new(a.x + t * ab.x, a.y + t * ab.y);
    (p.x - closest.x).hypot(p.y - closest.y)
}

// 计算动态画笔宽度
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn calculate_dynamic_width(
    base_width: f32,
    mode: DynamicBrushWidthMode,
    point_index: usize,
    total_points: usize,
    speed: Option<f32>,
) -> StrokeWidth {
    let width = match mode {
        DynamicBrushWidthMode::Disabled => return StrokeWidth::Fixed(base_width),

        DynamicBrushWidthMode::BrushTip => {
            // 模拟笔锋：在笔画末尾逐渐缩小
            let progress = point_index as f32 / total_points.max(1) as f32;
            // 在最后 30% 的笔画中逐渐缩小到 40% 的宽度
            if progress > 0.7 {
                let shrink_progress = (progress - 0.7) / 0.3; // 0.0 到 1.0
                base_width * (1.0 - shrink_progress * 0.6) // 从 100% 缩小到 40%
            } else {
                base_width
            }
        }

        DynamicBrushWidthMode::SpeedBased => {
            // 基于速度：速度快时变细，速度慢时变粗
            if let Some(speed_val) = speed {
                // 速度范围假设：0-500 像素/秒
                // 速度越快，宽度越小（最小到 50%）
                // 速度越慢，宽度越大（最大到 150%）
                let normalized_speed = (speed_val / 500.0).min(1.0);
                base_width * (1.5 - normalized_speed) // 从 150% 到 50%
            } else {
                base_width
            }
        }
    };
    StrokeWidth::Dynamic(vec![width])
}

// 插值算法 - 在点之间插入中间点
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn apply_point_interpolation_in_place(
    points: &mut Vec<Pos2>,
    width: &StrokeWidth,
    frequency: f32,
) -> StrokeWidth {
    if points.len() < 2 || frequency <= 0.0 {
        return width.clone();
    }

    match width {
        StrokeWidth::Fixed(w) => {
            let mut interpolated = Vec::with_capacity(points.len());

            for i in 0..points.len() - 1 {
                let p1 = points[i];
                let p2 = points[i + 1];
                interpolated.push(p1);

                let distance = p1.distance(p2);
                let num_interpolations = (distance * frequency) as usize;

                for j in 1..=num_interpolations {
                    let t = j as f32 / (num_interpolations + 1) as f32;
                    interpolated.push(Pos2::new(
                        p1.x + t * (p2.x - p1.x),
                        p1.y + t * (p2.y - p1.y),
                    ));
                }
            }

            if let Some(&last_point) = points.last() {
                interpolated.push(last_point);
            }

            *points = interpolated;
            StrokeWidth::Fixed(*w)
        }
        StrokeWidth::Dynamic(widths) => {
            let mut interpolated_points = Vec::with_capacity(points.len());
            let mut interpolated_widths = Vec::with_capacity(points.len());

            for i in 0..points.len() - 1 {
                let p1 = points[i];
                let p2 = points[i + 1];
                let width1 = widths[i.min(widths.len().saturating_sub(1))];
                let width2 = widths[(i + 1).min(widths.len().saturating_sub(1))];

                interpolated_points.push(p1);
                interpolated_widths.push(width1);

                let distance = p1.distance(p2);
                let num_interpolations = (distance * frequency) as usize;

                for j in 1..=num_interpolations {
                    let t = j as f32 / (num_interpolations + 1) as f32;
                    interpolated_points.push(Pos2::new(
                        p1.x + t * (p2.x - p1.x),
                        p1.y + t * (p2.y - p1.y),
                    ));
                    interpolated_widths.push(width1 + t * (width2 - width1));
                }
            }

            if let Some(&last_point) = points.last() {
                interpolated_points.push(last_point);
            }
            if let Some(&last_width) = widths.last() {
                interpolated_widths.push(last_width);
            }

            *points = interpolated_points;
            interpolated_widths.into()
        }
    }
}

// 判断笔画是否近似一条直线
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn is_stroke_linear(points: &[Pos2], tolerance: f32) -> bool {
    if points.len() < 3 {
        return true;
    }

    let a = points[0];
    let b = points[points.len() - 1];

    let ab = b - a;
    let ab_len = ab.length();

    // 起终点重合，无法定义直线
    if ab_len < f32::EPSILON {
        return false;
    }

    let mut max_dist: f32 = 0.0;

    for &p in &points[1..points.len() - 1] {
        let ap = p - a;
        // 2D 叉积的“模”
        let cross = ab.x * ap.y - ab.y * ap.x;
        let dist = cross.abs() / ab_len;
        max_dist = max_dist.max(dist);

        if max_dist > tolerance {
            return false;
        }
    }

    true
}

// 拉直笔画
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn straighten_stroke(points: &[Pos2], tolerance: f32) -> Vec<Pos2> {
    if is_stroke_linear(points, tolerance) {
        match points.len() {
            0 => Vec::new(),
            1 => vec![points[0]],
            _ => vec![points[0], points[points.len() - 1]],
        }
    } else {
        points.to_vec()
    }
}

pub fn draw_size_preview(painter: &Painter, pos: Pos2, size: f32) {
    const SIZE_PREVIEW_BORDER_WIDTH: f32 = 2.0;
    let radius = size / 2.0;
    painter.circle_filled(pos, radius, Color32::WHITE);
    painter.circle_stroke(
        pos,
        radius,
        Stroke::new(SIZE_PREVIEW_BORDER_WIDTH, Color32::BLACK),
    );
}

// 将图像调整大小以适应最大纹理大小限制
// 最大纹理大小通常为 2048x2048，如果图像超过此限制，将其缩放以适应
pub fn resize_image_for_texture(image: DynamicImage, max_texture_size: u32) -> DynamicImage {
    let (width, height) = image.dimensions();

    // 如果图像已经在限制内，直接返回
    if width <= max_texture_size && height <= max_texture_size {
        return image;
    }

    // 计算缩放比例以适应最大纹理大小
    let width_ratio = max_texture_size as f32 / width as f32;
    let height_ratio = max_texture_size as f32 / height as f32;
    let scale = width_ratio.min(height_ratio);

    let new_width = (width as f32 * scale) as u32;
    let new_height = (height as f32 * scale) as u32;

    // 确保新尺寸至少为 1x1
    let new_width = new_width.max(1);
    let new_height = new_height.max(1);

    // 使用缩放算法调整图像大小
    image.resize_exact(
        new_width,
        new_height,
        image::imageops::FilterType::CatmullRom,
    )
}

pub fn get_default_quick_colors() -> Vec<Color32> {
    vec![
        Color32::from_rgb(0, 0, 0),       // 黑色 - Primary text and outlines
        Color32::from_rgb(255, 255, 255), // 白色 - Highlighting and backgrounds
        Color32::from_rgb(0, 100, 255),   // 蓝色 - Diagrams and important information
        Color32::from_rgb(220, 20, 60),   // 红色 - Corrections and emphasis
        Color32::from_rgb(34, 139, 34),   // 绿色 - Positive feedback
        Color32::from_rgb(255, 140, 0),   // 橙色 - Secondary highlighting
    ]
}

pub fn get_default_canvas_color() -> Color32 {
    Color32::from_rgb(15, 38, 30)
}

// 绘制调整句柄
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn draw_resize_handles(painter: &egui::Painter, bbox: Rect) {
    let handle_size = 12.0;
    let handle_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    let handle_fill = Color32::BLUE;

    // 8个调整大小的句柄
    let handles = [
        (bbox.left_top(), TransformHandle::TopLeft),
        (bbox.right_top(), TransformHandle::TopRight),
        (bbox.left_bottom(), TransformHandle::BottomLeft),
        (bbox.right_bottom(), TransformHandle::BottomRight),
        (Pos2::new(bbox.center().x, bbox.top()), TransformHandle::Top),
        (
            Pos2::new(bbox.center().x, bbox.bottom()),
            TransformHandle::Bottom,
        ),
        (
            Pos2::new(bbox.left(), bbox.center().y),
            TransformHandle::Left,
        ),
        (
            Pos2::new(bbox.right(), bbox.center().y),
            TransformHandle::Right,
        ),
    ];

    for (pos, _) in &handles {
        let handle_rect = Rect::from_center_size(*pos, egui::vec2(handle_size, handle_size));
        painter.rect_filled(handle_rect, 0.0, handle_fill);
        painter.rect_stroke(handle_rect, 0.0, handle_stroke, egui::StrokeKind::Outside);
    }
}

// 获取鼠标位置下的调整句柄
pub fn get_transform_handle_at_pos(bbox: Rect, pos: Pos2) -> Option<TransformHandle> {
    let handle_size = 20.0;
    let handle_hit_size = handle_size * 1.5; // 扩大点击区域

    // 检查 8 个调整大小的句柄
    let handles = [
        (bbox.left_top(), TransformHandle::TopLeft),
        (bbox.right_top(), TransformHandle::TopRight),
        (bbox.left_bottom(), TransformHandle::BottomLeft),
        (bbox.right_bottom(), TransformHandle::BottomRight),
        (Pos2::new(bbox.center().x, bbox.top()), TransformHandle::Top),
        (
            Pos2::new(bbox.center().x, bbox.bottom()),
            TransformHandle::Bottom,
        ),
        (
            Pos2::new(bbox.left(), bbox.center().y),
            TransformHandle::Left,
        ),
        (
            Pos2::new(bbox.right(), bbox.center().y),
            TransformHandle::Right,
        ),
    ];

    for (handle_pos, handle_type) in &handles {
        let handle_rect =
            Rect::from_center_size(*handle_pos, egui::vec2(handle_hit_size, handle_hit_size));
        if handle_rect.contains(pos) {
            return Some(*handle_type);
        }
    }

    None
}

fn quad_bezier(p0: Pos2, p1: Pos2, p2: Pos2, t: f32) -> Pos2 {
    let u = 1.0 - t;
    Pos2::new(
        u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
        u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
    )
}

fn cubic_bezier(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let u = 1.0 - t;
    Pos2::new(
        u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
        u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
    )
}

pub fn rasterize_text(
    text: &crate::state::CanvasText,
    font_data: &[u8],
) -> Vec<crate::state::CanvasStroke> {
    let face = Face::parse(font_data, 0).unwrap();

    let mut strokes = Vec::new();
    let mut cursor_x = 0.0;

    let scale = text.font_size / face.units_per_em() as f32;

    for ch in text.text.chars() {
        if let Some(glyph_id) = face.glyph_index(ch) {
            let mut builder = StrokeBuilder {
                current: Vec::new(),
                strokes: Vec::new(),
                scale,
                offset: Pos2::new(text.pos.x + cursor_x, text.pos.y),
            };

            face.outline_glyph(glyph_id, &mut builder);

            for points in builder.strokes {
                strokes.push(CanvasStroke {
                    points,
                    width: StrokeWidth::Fixed(1.0),
                    color: text.color,
                    base_width: text.font_size,
                    shape: None,
                });
            }

            cursor_x += face.glyph_hor_advance(glyph_id).unwrap_or(0) as f32 * scale;
        }
    }

    strokes
}

struct StrokeBuilder {
    current: Vec<Pos2>,
    strokes: Vec<Vec<Pos2>>,
    scale: f32,
    offset: Pos2,
}

impl StrokeBuilder {
    #[inline]
    fn to_pos(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(
            self.offset.x + x * self.scale,
            self.offset.y - y * self.scale, // NOTE: flip Y for screen coords
        )
    }
}

impl OutlineBuilder for StrokeBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.current.len() > 1 {
            self.strokes.push(std::mem::take(&mut self.current));
        }
        self.current.push(self.to_pos(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push(self.to_pos(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = *self.current.last().unwrap();
        let p1 = self.to_pos(x1, y1);
        let p2 = self.to_pos(x, y);

        const STEPS: usize = 8;
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            self.current.push(quad_bezier(p0, p1, p2, t));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = *self.current.last().unwrap();
        let p1 = self.to_pos(x1, y1);
        let p2 = self.to_pos(x2, y2);
        let p3 = self.to_pos(x, y);

        const STEPS: usize = 12;
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            self.current.push(cubic_bezier(p0, p1, p2, p3, t));
        }
    }

    fn close(&mut self) {
        if self.current.len() > 1 {
            self.strokes.push(std::mem::take(&mut self.current));
        }
    }
}
