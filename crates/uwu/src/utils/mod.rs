pub mod associations;
pub mod dark_mode;
pub mod export;
pub mod plugins;
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

/// Maximum texture dimension used for GPU display. Original pixels are kept
/// separately for export, undo, and canvas files.
pub const MAX_TEXTURE_SIZE: u32 = 2048;

/// Creates a GPU texture from full-resolution RGBA data, downscaling only the
/// pixels uploaded to the GPU. The source `rgba` buffer is left untouched so
/// the original image detail survives display, export, and saving.
pub fn create_display_texture(
    ctx: &egui::Context,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> egui::TextureHandle {
    let (tex_data, tex_w, tex_h) = if width > MAX_TEXTURE_SIZE || height > MAX_TEXTURE_SIZE {
        let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
            .expect("invalid rgba dimensions for display texture");
        let resized =
            resize_image_for_texture(image::DynamicImage::ImageRgba8(img), MAX_TEXTURE_SIZE);
        let rgba = resized.to_rgba8();
        let (w, h) = rgba.dimensions();
        (rgba.into_raw(), w, h)
    } else {
        (rgba.to_vec(), width, height)
    };

    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [tex_w as usize, tex_h as usize],
        &tex_data,
    );
    ctx.load_texture("canvas_image", color_image, egui::TextureOptions::LINEAR)
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

/// Opens `path` with the system's default application for its file type.
pub fn open_with_default_app(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::PCWSTR;

        let file_w: Vec<u16> = OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let verb_w: Vec<u16> = OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let hinst = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(verb_w.as_ptr()),
                PCWSTR(file_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if hinst.0 as usize <= 32 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "opening files with the default application is unsupported on this platform",
        ))
    }
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
// ===== Polygon geometry utilities for lasso selection =====

/// Determines if a point is inside a polygon using the ray casting algorithm.
/// Returns `true` if the point is inside or on the boundary.
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn point_in_polygon(point: Pos2, polygon: &[Pos2]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let pi = polygon[i];
        let pj = polygon[j];
        if ((pi.y > point.y) != (pj.y > point.y))
            && (point.x < (pj.x - pi.x) * (point.y - pi.y) / (pj.y - pi.y) + pi.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Checks whether any polygon edge crosses any rect edge, or if either shape
/// contains a corner of the other. Used for `MarqueeMatchMode::Overlapping`.
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn polygon_intersects_rect(polygon: &[Pos2], rect: Rect) -> bool {
    if polygon.len() < 3 {
        return false;
    }

    // Fast check: any polygon vertex inside the rect?
    if polygon.iter().any(|p| rect.contains(*p)) {
        return true;
    }

    let corners = [
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right(), rect.top()),
        Pos2::new(rect.right(), rect.bottom()),
        Pos2::new(rect.left(), rect.bottom()),
    ];

    // Any rect corner inside the polygon?
    if corners.iter().any(|c| point_in_polygon(*c, polygon)) {
        return true;
    }

    // Check edge intersections between polygon and rectangle
    let rect_edges = [
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[3]),
        (corners[3], corners[0]),
    ];

    let n = polygon.len();
    for i in 0..n {
        let a = polygon[i];
        let b = polygon[(i + 1) % n];
        for &(c, d) in &rect_edges {
            if segments_intersect(a, b, c, d) {
                return true;
            }
        }
    }

    false
}

/// Checks whether all four corners of `rect` lie inside `polygon`.
/// Used for `MarqueeMatchMode::Containing`.
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn polygon_contains_rect(polygon: &[Pos2], rect: Rect) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let corners = [
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right(), rect.top()),
        Pos2::new(rect.right(), rect.bottom()),
        Pos2::new(rect.left(), rect.bottom()),
    ];
    corners.iter().all(|c| point_in_polygon(*c, polygon))
}

/// Ear-clipping triangulation for arbitrary simple (concave) polygons.
/// Returns a vector of triangles, each as `[Pos2; 3]`.
/// The polygon must be non-self-intersecting (simple).
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn triangulate_polygon(polygon: &[Pos2]) -> Vec<[Pos2; 3]> {
    let n = polygon.len();
    if n < 3 {
        return Vec::new();
    }

    // Index list that we'll clip ears from
    let mut indices: Vec<usize> = (0..n).collect();
    let mut triangles: Vec<[Pos2; 3]> = Vec::with_capacity(n.saturating_sub(2));

    // Determine polygon orientation from signed area
    let mut area: f32 = 0.0;
    let mut j = n - 1;
    for i in 0..n {
        area += (polygon[j].x + polygon[i].x) * (polygon[j].y - polygon[i].y);
        j = i;
    }
    let is_ccw = area < 0.0;

    let mut iterations = 0;
    // Allow enough iterations for worst-case O(n²)
    let max_iterations = n * n;

    let mut i = 0usize;
    while indices.len() > 3 && iterations < max_iterations {
        iterations += 1;
        let len = indices.len();
        let prev = indices[(i + len - 1) % len];
        let curr = indices[i];
        let next = indices[(i + 1) % len];

        let a = polygon[prev];
        let b = polygon[curr];
        let c = polygon[next];

        // Cross product of edge (prev→curr) and (curr→next)
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);

        // Ear vertex must be convex (same sign as polygon orientation)
        let convex = if is_ccw { cross > 0.0 } else { cross < 0.0 };
        if !convex {
            i = (i + 1) % len;
            continue;
        }

        // Check no other vertex lies inside triangle (a, b, c)
        let mut is_ear = true;
        for &vi in &indices {
            if vi == prev || vi == curr || vi == next {
                continue;
            }
            if point_in_triangle(polygon[vi], a, b, c) {
                is_ear = false;
                break;
            }
        }

        if is_ear {
            triangles.push([a, b, c]);
            indices.remove(i);
            i = i.saturating_sub(1);
        } else {
            i = (i + 1) % len;
        }
    }

    // Final triangle
    if indices.len() == 3 {
        triangles.push([
            polygon[indices[0]],
            polygon[indices[1]],
            polygon[indices[2]],
        ]);
    }

    triangles
}

/// Ramer-Douglas-Peucker simplification reduces vertex count
/// while preserving the overall shape.
/// `epsilon` is the maximum distance (in canvas coordinates) a simplified
/// edge can deviate from the original polyline.
#[cfg_attr(feature = "profiling", profiling::function)]
pub fn simplify_polygon(points: &[Pos2], epsilon: f32) -> Vec<Pos2> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }

    // Find the point with the maximum distance from the line segment (start, end)
    let start = points[0];
    let end = points[n - 1];
    let (mut dmax, mut idx) = (0.0f32, 0usize);

    for (i, p) in points.iter().enumerate().skip(1) {
        let d = perpendicular_distance(*p, start, end);
        if d > dmax {
            dmax = d;
            idx = i;
        }
    }

    let mut result = Vec::new();
    if dmax > epsilon {
        // Recursively simplify both halves
        let left = simplify_polygon(&points[..=idx], epsilon);
        let right = simplify_polygon(&points[idx..], epsilon);
        // Combine: left + right[1..] (avoid duplicating the split point)
        result.reserve(left.len() + right.len() - 1);
        result.extend_from_slice(&left[..left.len() - 1]);
        result.extend_from_slice(&right);
    } else {
        result.push(start);
        result.push(end);
    }

    result
}

// ---- Private helpers ----

/// Cross-product-based orientation: positive = CCW turn, negative = CW, zero = collinear
fn orientation(a: Pos2, b: Pos2, c: Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Check if two line segments (a→b) and (c→d) intersect (excluding collinear overlaps)
fn segments_intersect(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> bool {
    let o1 = orientation(a, b, c);
    let o2 = orientation(a, b, d);
    let o3 = orientation(c, d, a);
    let o4 = orientation(c, d, b);

    // General case: endpoints straddle each other's line
    (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0)
}

/// Check if point `p` lies inside triangle (a, b, c) using barycentric coordinates.
/// Returns `true` for strictly interior points (not on edge).
fn point_in_triangle(p: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let d = orientation(a, b, p);
    let e = orientation(b, c, p);
    let f = orientation(c, a, p);

    let has_neg = d < 0.0 || e < 0.0 || f < 0.0;
    let has_pos = d > 0.0 || e > 0.0 || f > 0.0;

    !(has_neg && has_pos)
}

/// Perpendicular distance from point `p` to the line through `a` and `b`.
fn perpendicular_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length_sq = dx * dx + dy * dy;
    if length_sq == 0.0 {
        return p.distance(a);
    }
    let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / length_sq;
    let t = t.clamp(0.0, 1.0);
    let proj = Pos2::new(a.x + t * dx, a.y + t * dy);
    p.distance(proj)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triangulate_polygon_ccw_convex() {
        let polygon = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 100.0),
            Pos2::new(100.0, 100.0),
            Pos2::new(100.0, 0.0),
        ];
        let triangles = triangulate_polygon(&polygon);
        assert_eq!(triangles.len(), 2);
    }

    #[test]
    fn test_triangulate_polygon_cw_convex() {
        let polygon = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(100.0, 0.0),
            Pos2::new(100.0, 100.0),
            Pos2::new(0.0, 100.0),
        ];
        let triangles = triangulate_polygon(&polygon);
        assert_eq!(triangles.len(), 2);
    }

    #[test]
    fn test_triangulate_polygon_concave() {
        // L-shaped concave polygon (CCW)
        let polygon = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 200.0),
            Pos2::new(100.0, 200.0),
            Pos2::new(100.0, 100.0),
            Pos2::new(200.0, 100.0),
            Pos2::new(200.0, 0.0),
        ];
        let triangles = triangulate_polygon(&polygon);
        assert_eq!(triangles.len(), 4);
    }
}
