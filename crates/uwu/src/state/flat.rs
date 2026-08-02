use bitcode::{Decode, Encode};

use super::{
    CanvasImage, CanvasObject, CanvasShape, CanvasShapeType, CanvasState, CanvasStroke, CanvasText,
    Color32, History, HistoryCommand, ObjectTransform, PageState, Pos2, StrokeWidth,
};

// ===== Flat data types for bitcode serialization =====

#[derive(Encode, Decode, Debug, Clone)]
pub struct PageStateFlat {
    pub canvas: CanvasStateFlat,
    pub history: HistoryFlat,
    pub view_offset: [f32; 2],
    pub view_zoom: f32,
}

impl PageStateFlat {
    /// Validates structural invariants that runtime code relies on. Corrupt or
    /// hand-crafted files are rejected here instead of panicking later during
    /// painting or texture upload.
    pub fn validate(&self) -> Result<(), String> {
        if !self.view_offset[0].is_finite()
            || !self.view_offset[1].is_finite()
            || !self.view_zoom.is_finite()
            || self.view_zoom <= 0.0
        {
            return Err("view state contains non-finite or invalid values".into());
        }

        for (index, object) in self.canvas.objects.iter().enumerate() {
            validate_object(object).map_err(|e| format!("object {index}: {e}"))?;
        }

        for (index, command) in self.history.undo_stack.iter().enumerate() {
            validate_history_command(command)
                .map_err(|e| format!("undo history command {index}: {e}"))?;
        }
        for (index, command) in self.history.redo_stack.iter().enumerate() {
            validate_history_command(command)
                .map_err(|e| format!("redo history command {index}: {e}"))?;
        }

        Ok(())
    }
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct CanvasStateFlat {
    pub objects: Vec<CanvasObjectFlat>,
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum CanvasObjectFlat {
    Stroke(StrokeFlat),
    Text(TextFlat),
    Shape(ShapeFlat),
    Image(ImageFlat),
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct StrokeFlat {
    pub points: Vec<[f32; 2]>,
    pub width: StrokeWidthFlat,
    pub color: [u8; 4],
    pub base_width: f32,
    pub shape: Option<ShapeTypeFlat>,
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum StrokeWidthFlat {
    Fixed(f32),
    Dynamic(Vec<f32>),
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct TextFlat {
    pub text: String,
    pub pos: [f32; 2],
    pub color: [u8; 4],
    pub font_size: f32,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct ShapeFlat {
    pub shape_type: ShapeTypeFlat,
    pub pos: [f32; 2],
    pub size: f32,
    pub color: [u8; 4],
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum ShapeTypeFlat {
    Line,
    Arrow,
    Rectangle,
    Triangle,
    Circle,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct ImageFlat {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub aspect_ratio: f32,
    pub image_data: Vec<u8>,
    pub image_size: [u32; 2],
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct HistoryFlat {
    pub undo_stack: Vec<HistoryCommandFlat>,
    pub redo_stack: Vec<HistoryCommandFlat>,
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum HistoryCommandFlat {
    AddObject {
        index: u32,
        object: CanvasObjectFlat,
    },
    RemoveObject {
        index: u32,
        object: CanvasObjectFlat,
    },
    ClearObjects {
        objects: Vec<CanvasObjectFlat>,
    },
    MoveObject {
        index: u32,
        old_position: [f32; 2],
        new_position: [f32; 2],
    },
    TransformObject {
        index: u32,
        old_transform: ObjectTransformFlat,
        new_transform: ObjectTransformFlat,
    },
    ReplaceObjects {
        old: Vec<CanvasObjectFlat>,
        new: Vec<CanvasObjectFlat>,
    },
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct ObjectTransformFlat {
    pub pos: [f32; 2],
    pub size: [f32; 2],
}

// ===== Helper: convert individual CanvasObject ↔ CanvasObjectFlat =====

fn canvas_object_to_flat(obj: &CanvasObject) -> CanvasObjectFlat {
    match obj {
        CanvasObject::Stroke(s) => CanvasObjectFlat::Stroke(StrokeFlat {
            points: s.points.iter().map(|p| [p.x, p.y]).collect(),
            width: match &s.width {
                StrokeWidth::Fixed(w) => StrokeWidthFlat::Fixed(*w),
                StrokeWidth::Dynamic(v) => StrokeWidthFlat::Dynamic(v.clone()),
            },
            color: [s.color.r(), s.color.g(), s.color.b(), s.color.a()],
            base_width: s.base_width,
            shape: s.shape.map(|shape_type| match shape_type {
                CanvasShapeType::Line => ShapeTypeFlat::Line,
                CanvasShapeType::Arrow => ShapeTypeFlat::Arrow,
                CanvasShapeType::Rectangle => ShapeTypeFlat::Rectangle,
                CanvasShapeType::Triangle => ShapeTypeFlat::Triangle,
                CanvasShapeType::Circle => ShapeTypeFlat::Circle,
            }),
        }),
        CanvasObject::Text(t) => CanvasObjectFlat::Text(TextFlat {
            text: t.text.clone(),
            pos: [t.pos.x, t.pos.y],
            color: [t.color.r(), t.color.g(), t.color.b(), t.color.a()],
            font_size: t.font_size,
        }),
        CanvasObject::Shape(s) => CanvasObjectFlat::Shape(ShapeFlat {
            shape_type: match s.shape_type {
                CanvasShapeType::Line => ShapeTypeFlat::Line,
                CanvasShapeType::Arrow => ShapeTypeFlat::Arrow,
                CanvasShapeType::Rectangle => ShapeTypeFlat::Rectangle,
                CanvasShapeType::Triangle => ShapeTypeFlat::Triangle,
                CanvasShapeType::Circle => ShapeTypeFlat::Circle,
            },
            pos: [s.pos.x, s.pos.y],
            size: s.size,
            color: [s.color.r(), s.color.g(), s.color.b(), s.color.a()],
        }),
        CanvasObject::Image(img) => {
            let data: Vec<u8> = img.image_data.to_vec();
            CanvasObjectFlat::Image(ImageFlat {
                pos: [img.pos.x, img.pos.y],
                size: [img.size.x, img.size.y],
                aspect_ratio: img.aspect_ratio,
                image_data: data,
                image_size: img.image_size,
            })
        }
    }
}

fn object_to_canvas_object(obj: CanvasObjectFlat, ctx: &egui::Context) -> CanvasObject {
    match obj {
        CanvasObjectFlat::Stroke(s) => CanvasObject::Stroke(CanvasStroke {
            points: s
                .points
                .into_iter()
                .map(|p| Pos2::new(p[0], p[1]))
                .collect(),
            width: match s.width {
                StrokeWidthFlat::Fixed(w) => StrokeWidth::Fixed(w),
                StrokeWidthFlat::Dynamic(v) => StrokeWidth::Dynamic(v),
            },
            color: Color32::from_rgba_unmultiplied(s.color[0], s.color[1], s.color[2], s.color[3]),
            base_width: s.base_width,
            shape: s.shape.map(|shape_type| match shape_type {
                ShapeTypeFlat::Line => CanvasShapeType::Line,
                ShapeTypeFlat::Arrow => CanvasShapeType::Arrow,
                ShapeTypeFlat::Rectangle => CanvasShapeType::Rectangle,
                ShapeTypeFlat::Triangle => CanvasShapeType::Triangle,
                ShapeTypeFlat::Circle => CanvasShapeType::Circle,
            }),
            cached_bbox: std::cell::Cell::new(None),
        }),
        CanvasObjectFlat::Text(t) => CanvasObject::Text(CanvasText {
            text: t.text,
            pos: Pos2::new(t.pos[0], t.pos[1]),
            color: Color32::from_rgba_unmultiplied(t.color[0], t.color[1], t.color[2], t.color[3]),
            font_size: t.font_size,
            cached_size: std::cell::Cell::new(None),
        }),
        CanvasObjectFlat::Shape(s) => CanvasObject::Shape(CanvasShape {
            shape_type: match s.shape_type {
                ShapeTypeFlat::Line => CanvasShapeType::Line,
                ShapeTypeFlat::Arrow => CanvasShapeType::Arrow,
                ShapeTypeFlat::Rectangle => CanvasShapeType::Rectangle,
                ShapeTypeFlat::Triangle => CanvasShapeType::Triangle,
                ShapeTypeFlat::Circle => CanvasShapeType::Circle,
            },
            pos: Pos2::new(s.pos[0], s.pos[1]),
            size: s.size,
            color: Color32::from_rgba_unmultiplied(s.color[0], s.color[1], s.color[2], s.color[3]),
        }),
        CanvasObjectFlat::Image(img) => {
            let width = img.image_size[0] as usize;
            let height = img.image_size[1] as usize;
            let rgba_data = img.image_data;
            // Keep the original pixels for export/save; only the GPU texture is
            // downscaled for display.
            let texture = crate::utils::create_display_texture(
                ctx,
                &rgba_data,
                width as u32,
                height as u32,
            );
            let image_data: std::sync::Arc<[u8]> = rgba_data.into();
            CanvasObject::Image(CanvasImage {
                texture,
                pos: Pos2::new(img.pos[0], img.pos[1]),
                size: egui::Vec2::new(img.size[0], img.size[1]),
                aspect_ratio: img.aspect_ratio,
                image_data,
                image_size: img.image_size,
            })
        }
    }
}

fn validate_object(obj: &CanvasObjectFlat) -> Result<(), String> {
    let finite_pos = |p: &[f32; 2]| p[0].is_finite() && p[1].is_finite();

    match obj {
        CanvasObjectFlat::Stroke(s) => {
            if s.points.is_empty() {
                return Err("stroke has no points".into());
            }
            if !s.points.iter().all(finite_pos) {
                return Err("stroke contains non-finite points".into());
            }
            match &s.width {
                StrokeWidthFlat::Fixed(w) => {
                    if !w.is_finite() || *w < 0.0 {
                        return Err("stroke has an invalid fixed width".into());
                    }
                }
                StrokeWidthFlat::Dynamic(widths) => {
                    if widths.is_empty() {
                        return Err("stroke has an empty dynamic width list".into());
                    }
                    if widths.len() != s.points.len() {
                        return Err(format!(
                            "stroke dynamic width count {} does not match point count {}",
                            widths.len(),
                            s.points.len()
                        ));
                    }
                    if !widths.iter().all(|w| w.is_finite() && *w >= 0.0) {
                        return Err("stroke contains invalid dynamic widths".into());
                    }
                }
            }
            if !s.base_width.is_finite() || s.base_width < 0.0 {
                return Err("stroke has an invalid base width".into());
            }
        }
        CanvasObjectFlat::Text(t) => {
            if !finite_pos(&t.pos) || !t.font_size.is_finite() || t.font_size <= 0.0 {
                return Err("text has invalid position or font size".into());
            }
        }
        CanvasObjectFlat::Shape(s) => {
            if !finite_pos(&s.pos) || !s.size.is_finite() || s.size < 0.0 {
                return Err("shape has invalid position or size".into());
            }
        }
        CanvasObjectFlat::Image(img) => {
            let [width, height] = img.image_size;
            let expected = (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(4));
            if width == 0 || height == 0 {
                return Err("image has a zero dimension".into());
            }
            if expected != Some(img.image_data.len()) {
                return Err(format!(
                    "image data length {} does not match dimensions {width}x{height}",
                    img.image_data.len()
                ));
            }
            if !finite_pos(&img.pos)
                || !img.size[0].is_finite()
                || !img.size[1].is_finite()
                || img.size[0] <= 0.0
                || img.size[1] <= 0.0
            {
                return Err("image has invalid position or size".into());
            }
            if !img.aspect_ratio.is_finite() || img.aspect_ratio <= 0.0 {
                return Err("image has an invalid aspect ratio".into());
            }
        }
    }
    Ok(())
}

fn validate_history_command(command: &HistoryCommandFlat) -> Result<(), String> {
    let finite_pos = |p: &[f32; 2]| p[0].is_finite() && p[1].is_finite();
    let finite_transform = |t: &ObjectTransformFlat| {
        finite_pos(&t.pos) && t.size[0].is_finite() && t.size[1].is_finite()
    };

    match command {
        HistoryCommandFlat::AddObject { object, .. }
        | HistoryCommandFlat::RemoveObject { object, .. } => validate_object(object),
        HistoryCommandFlat::ClearObjects { objects } => {
            for (index, object) in objects.iter().enumerate() {
                validate_object(object).map_err(|e| format!("clear object {index}: {e}"))?;
            }
            Ok(())
        }
        HistoryCommandFlat::MoveObject {
            old_position,
            new_position,
            ..
        } => {
            if !finite_pos(old_position) || !finite_pos(new_position) {
                Err("move command contains non-finite positions".into())
            } else {
                Ok(())
            }
        }
        HistoryCommandFlat::TransformObject {
            old_transform,
            new_transform,
            ..
        } => {
            if !finite_transform(old_transform) || !finite_transform(new_transform) {
                Err("transform command contains non-finite values".into())
            } else {
                Ok(())
            }
        }
        HistoryCommandFlat::ReplaceObjects { old, new } => {
            for (index, object) in old.iter().chain(new).enumerate() {
                validate_object(object).map_err(|e| format!("replaced object {index}: {e}"))?;
            }
            Ok(())
        }
    }
}

// ===== Conversions: runtime → flat =====

impl From<&CanvasState> for CanvasStateFlat {
    fn from(state: &CanvasState) -> Self {
        CanvasStateFlat {
            objects: state.objects.iter().map(canvas_object_to_flat).collect(),
        }
    }
}

impl From<&ObjectTransform> for ObjectTransformFlat {
    fn from(t: &ObjectTransform) -> Self {
        ObjectTransformFlat {
            pos: [t.pos.x, t.pos.y],
            size: [t.size.x, t.size.y],
        }
    }
}

fn history_command_to_flat(cmd: &HistoryCommand) -> HistoryCommandFlat {
    match cmd {
        HistoryCommand::AddObject { index, object } => HistoryCommandFlat::AddObject {
            index: *index as u32,
            object: canvas_object_to_flat(object),
        },
        HistoryCommand::RemoveObject { index, object } => HistoryCommandFlat::RemoveObject {
            index: *index as u32,
            object: canvas_object_to_flat(object),
        },
        HistoryCommand::ClearObjects { objects } => HistoryCommandFlat::ClearObjects {
            objects: objects.iter().map(canvas_object_to_flat).collect(),
        },
        HistoryCommand::MoveObject {
            index,
            old_position,
            new_position,
        } => HistoryCommandFlat::MoveObject {
            index: *index as u32,
            old_position: [old_position.x, old_position.y],
            new_position: [new_position.x, new_position.y],
        },
        HistoryCommand::TransformObject {
            index,
            old_transform,
            new_transform,
        } => HistoryCommandFlat::TransformObject {
            index: *index as u32,
            old_transform: ObjectTransformFlat::from(old_transform),
            new_transform: ObjectTransformFlat::from(new_transform),
        },
        HistoryCommand::ReplaceObjects { old, new } => HistoryCommandFlat::ReplaceObjects {
            old: old.iter().map(canvas_object_to_flat).collect(),
            new: new.iter().map(canvas_object_to_flat).collect(),
        },
        // BatchCommand cannot be represented in the flat format: bitcode's
        // derive does not support recursive types. Expansion happens in
        // `From<&History> for HistoryFlat`; reaching this arm means a batch
        // was converted directly, which must not happen.
        HistoryCommand::BatchCommand { .. } => {
            unreachable!("BatchCommand must be flattened before conversion")
        }
    }
}

impl From<&History> for HistoryFlat {
    fn from(history: &History) -> Self {
        // Batch commands expand to their inner commands. For the undo stack the
        // inner order is kept (undo pops in reverse), while for the redo stack
        // the inner order is reversed (redo pops the last entry first).
        fn flatten(cmd: &HistoryCommand, for_redo: bool) -> Vec<HistoryCommandFlat> {
            match cmd {
                HistoryCommand::BatchCommand { commands } => {
                    let mut inner: Vec<HistoryCommandFlat> =
                        commands.iter().flat_map(|c| flatten(c, for_redo)).collect();
                    if for_redo {
                        inner.reverse();
                    }
                    inner
                }
                other => vec![history_command_to_flat(other)],
            }
        }

        HistoryFlat {
            undo_stack: history
                .undo_stack
                .iter()
                .flat_map(|cmd| flatten(cmd, false))
                .collect(),
            redo_stack: history
                .redo_stack
                .iter()
                .flat_map(|cmd| flatten(cmd, true))
                .collect(),
        }
    }
}

impl From<&PageState> for PageStateFlat {
    fn from(state: &PageState) -> Self {
        PageStateFlat {
            canvas: CanvasStateFlat::from(&state.canvas),
            history: HistoryFlat::from(&state.history),
            view_offset: [state.view_offset.x, state.view_offset.y],
            view_zoom: state.view_zoom,
        }
    }
}

// ===== Conversions: flat → runtime =====

impl PageState {
    /// Loads a PageState from a file, using the egui context
    /// to create GPU textures for any deserialized images.
    pub fn load_from_file(
        path: &std::path::PathBuf,
        ctx: &egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(path)?;

        const HEADER_SIZE: usize = 4;
        if bytes.len() < HEADER_SIZE
            || bytes[..3] != *super::CANVAS_FILE_MAGIC
            || bytes[3] != super::CANVAS_FILE_VERSION
        {
            let actual = if bytes.len() >= 4 {
                format!("magic={:02x?}, version={}", &bytes[..3], bytes[3])
            } else {
                format!("too short ({} bytes)", bytes.len())
            };
            return Err(format!(
                "unsupported canvas file format: expected magic=UWU, version={}, got {actual}",
                super::CANVAS_FILE_VERSION
            )
            .into());
        }

        let payload = &bytes[HEADER_SIZE..];
        let flat =
            bitcode::decode::<PageStateFlat>(payload).map_err(|e| format!("bitcode error: {e}"))?;

        flat.validate()
            .map_err(|e| format!("invalid canvas file: {e}"))?;

        Ok(Self::from_flat(flat, ctx))
    }

    fn from_flat(flat: PageStateFlat, ctx: &egui::Context) -> Self {
        PageState {
            canvas: CanvasState {
                objects: flat
                    .canvas
                    .objects
                    .into_iter()
                    .map(|obj| object_to_canvas_object(obj, ctx))
                    .collect(),
            },
            history: History {
                undo_stack: flat
                    .history
                    .undo_stack
                    .into_iter()
                    .map(|cmd| history_command_to_runtime(cmd, ctx))
                    .collect(),
                redo_stack: flat
                    .history
                    .redo_stack
                    .into_iter()
                    .map(|cmd| history_command_to_runtime(cmd, ctx))
                    .collect(),
                max_history_size: 50,
            },
            view_offset: egui::Vec2::new(flat.view_offset[0], flat.view_offset[1]),
            view_zoom: flat.view_zoom,
        }
    }

    /// Shows a file-open dialog and loads a PageState.
    pub fn load_from_file_with_dialog(
        ctx: &egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = rfd::FileDialog::new()
            .add_filter("画布文件", &[super::CANVAS_FILE_EXT])
            .pick_file()
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidFilename,
                "已取消",
            ))?;
        Self::load_from_file(&path, ctx)
    }

    /// Saves the page state to a file using bitcode binary format.
    pub fn save_to_file(
        &self,
        path: &std::path::PathBuf,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let flat = PageStateFlat::from(self);
        let payload = bitcode::encode(&flat);

        let header = super::make_canvas_file_header();
        const HEADER_SIZE: usize = 4;
        let mut out = Vec::with_capacity(HEADER_SIZE + payload.len());
        out.extend_from_slice(&header);
        out.extend_from_slice(payload.as_slice());

        // Write to a temporary file in the same directory and rename it, so a
        // crash mid-write cannot leave a truncated/corrupt canvas file.
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("canvas.owo");
        let tmp_path = path.with_file_name(format!("{file_name}.tmp"));
        std::fs::write(&tmp_path, out)?;
        if let Err(err) = std::fs::rename(&tmp_path, path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(err.into());
        }
        Ok(())
    }

    /// Shows a file-save dialog and saves the page state.
    pub fn save_to_file_with_dialog(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = rfd::FileDialog::new()
            .add_filter("画布文件", &[super::CANVAS_FILE_EXT])
            .set_file_name(format!("canvas.{}", super::CANVAS_FILE_EXT))
            .save_file()
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidFilename,
                "已取消",
            ))?;
        self.save_to_file(&path)?;
        Ok(())
    }
}

fn history_command_to_runtime(cmd: HistoryCommandFlat, ctx: &egui::Context) -> HistoryCommand {
    match cmd {
        HistoryCommandFlat::AddObject { index, object } => HistoryCommand::AddObject {
            index: index as usize,
            object: object_to_canvas_object(object, ctx),
        },
        HistoryCommandFlat::RemoveObject { index, object } => HistoryCommand::RemoveObject {
            index: index as usize,
            object: object_to_canvas_object(object, ctx),
        },
        HistoryCommandFlat::ClearObjects { objects } => HistoryCommand::ClearObjects {
            objects: objects
                .into_iter()
                .map(|obj| object_to_canvas_object(obj, ctx))
                .collect(),
        },
        HistoryCommandFlat::MoveObject {
            index,
            old_position,
            new_position,
        } => HistoryCommand::MoveObject {
            index: index as usize,
            old_position: egui::Vec2::new(old_position[0], old_position[1]),
            new_position: egui::Vec2::new(new_position[0], new_position[1]),
        },
        HistoryCommandFlat::TransformObject {
            index,
            old_transform,
            new_transform,
        } => HistoryCommand::TransformObject {
            index: index as usize,
            old_transform: ObjectTransform {
                pos: Pos2::new(old_transform.pos[0], old_transform.pos[1]),
                size: egui::Vec2::new(old_transform.size[0], old_transform.size[1]),
            },
            new_transform: ObjectTransform {
                pos: Pos2::new(new_transform.pos[0], new_transform.pos[1]),
                size: egui::Vec2::new(new_transform.size[0], new_transform.size[1]),
            },
        },
        HistoryCommandFlat::ReplaceObjects { old, new } => HistoryCommand::ReplaceObjects {
            old: old
                .into_iter()
                .map(|obj| object_to_canvas_object(obj, ctx))
                .collect(),
            new: new
                .into_iter()
                .map(|obj| object_to_canvas_object(obj, ctx))
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CanvasShape, CanvasShapeType};
    use egui::{Color32, Pos2};

    #[test]
    fn test_canvas_object_to_flat_shape() {
        let shape = CanvasObject::Shape(CanvasShape {
            shape_type: CanvasShapeType::Circle,
            pos: Pos2::new(10.0, 20.0),
            size: 5.0,
            color: Color32::from_rgba_unmultiplied(255, 0, 0, 255),
        });

        let flat = canvas_object_to_flat(&shape);

        if let CanvasObjectFlat::Shape(flat_shape) = flat {
            assert!(matches!(flat_shape.shape_type, ShapeTypeFlat::Circle));
            assert_eq!(flat_shape.pos, [10.0, 20.0]);
            assert_eq!(flat_shape.size, 5.0);
            assert_eq!(flat_shape.color, [255, 0, 0, 255]);
        } else {
            panic!("Expected ShapeFlat");
        }
    }

    #[test]
    fn test_canvas_object_to_flat_text() {
        let text = CanvasObject::Text(crate::state::CanvasText {
            text: "Hello".to_string(),
            pos: Pos2::new(1.0, 2.0),
            color: Color32::from_rgba_unmultiplied(0, 255, 0, 255),
            font_size: 14.0,
            cached_size: std::cell::Cell::new(None),
        });

        let flat = canvas_object_to_flat(&text);

        if let CanvasObjectFlat::Text(flat_text) = flat {
            assert_eq!(flat_text.text, "Hello");
            assert_eq!(flat_text.pos, [1.0, 2.0]);
            assert_eq!(flat_text.font_size, 14.0);
            assert_eq!(flat_text.color, [0, 255, 0, 255]);
        } else {
            panic!("Expected TextFlat");
        }
    }

    #[test]
    fn test_stroke_shape_round_trip() {
        let stroke = CanvasObject::Stroke(CanvasStroke {
            points: vec![Pos2::new(0.0, 0.0), Pos2::new(10.0, 10.0)],
            width: StrokeWidth::Fixed(3.0),
            color: Color32::WHITE,
            base_width: 3.0,
            shape: Some(CanvasShapeType::Rectangle),
            cached_bbox: std::cell::Cell::new(None),
        });

        let flat = canvas_object_to_flat(&stroke);
        let ctx = egui::Context::default();
        let back = object_to_canvas_object(flat, &ctx);

        if let CanvasObject::Stroke(restored) = back {
            assert_eq!(restored.shape, Some(CanvasShapeType::Rectangle));
        } else {
            panic!("Expected Stroke");
        }
    }

    #[test]
    fn test_batch_and_replace_history_round_trip() {
        let obj = |shape_type| {
            CanvasObject::Shape(CanvasShape {
                shape_type,
                pos: Pos2::new(0.0, 0.0),
                size: 10.0,
                color: Color32::WHITE,
            })
        };
        let history = History {
            undo_stack: vec![
                HistoryCommand::BatchCommand {
                    commands: vec![
                        HistoryCommand::RemoveObject {
                            index: 2,
                            object: obj(CanvasShapeType::Circle),
                        },
                        HistoryCommand::AddObject {
                            index: 3,
                            object: obj(CanvasShapeType::Triangle),
                        },
                    ],
                },
                HistoryCommand::ReplaceObjects {
                    old: vec![obj(CanvasShapeType::Line)],
                    new: vec![obj(CanvasShapeType::Arrow), obj(CanvasShapeType::Circle)],
                },
            ],
            redo_stack: vec![HistoryCommand::BatchCommand {
                commands: vec![
                    HistoryCommand::RemoveObject {
                        index: 2,
                        object: obj(CanvasShapeType::Circle),
                    },
                    HistoryCommand::AddObject {
                        index: 3,
                        object: obj(CanvasShapeType::Triangle),
                    },
                ],
            }],
            max_history_size: 50,
        };

        let flat = HistoryFlat::from(&history);
        let bytes = bitcode::encode(&flat);
        let decoded: HistoryFlat = bitcode::decode(&bytes).unwrap();
        let ctx = egui::Context::default();
        let back = History {
            undo_stack: decoded
                .undo_stack
                .into_iter()
                .map(|cmd| history_command_to_runtime(cmd, &ctx))
                .collect(),
            redo_stack: decoded
                .redo_stack
                .into_iter()
                .map(|cmd| history_command_to_runtime(cmd, &ctx))
                .collect(),
            max_history_size: 50,
        };

        // The batch on the undo stack expands in inner order: RemoveObject
        // first (undo pops it last), then AddObject and the ReplaceObjects
        // command that was pushed after the batch.
        assert_eq!(back.undo_stack.len(), 3);
        assert!(matches!(
            back.undo_stack[0],
            HistoryCommand::RemoveObject { index: 2, .. }
        ));
        assert!(matches!(
            back.undo_stack[1],
            HistoryCommand::AddObject { index: 3, .. }
        ));
        assert!(matches!(
            back.undo_stack[2],
            HistoryCommand::ReplaceObjects { .. }
        ));

        // The batch on the redo stack expands in reversed inner order so that
        // popping the stack applies RemoveObject before AddObject again.
        assert_eq!(back.redo_stack.len(), 2);
        assert!(matches!(
            back.redo_stack[0],
            HistoryCommand::AddObject { index: 3, .. }
        ));
        assert!(matches!(
            back.redo_stack[1],
            HistoryCommand::RemoveObject { index: 2, .. }
        ));
    }

    #[test]
    fn test_page_state_save_load_round_trip() {
        let ctx = egui::Context::default();
        let stroke = CanvasObject::Stroke(CanvasStroke {
            points: vec![Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)],
            width: StrokeWidth::Fixed(3.0),
            color: Color32::WHITE,
            base_width: 3.0,
            shape: Some(CanvasShapeType::Rectangle),
            cached_bbox: std::cell::Cell::new(None),
        });
        let page = PageState {
            canvas: CanvasState {
                objects: vec![stroke],
            },
            history: History {
                undo_stack: vec![HistoryCommand::ReplaceObjects {
                    old: Vec::new(),
                    new: Vec::new(),
                }],
                redo_stack: Vec::new(),
                max_history_size: 50,
            },
            view_offset: egui::Vec2::new(1.0, 2.0),
            view_zoom: 1.5,
        };

        let path = std::env::temp_dir().join(format!("uwu_flat_test_{}.owo", std::process::id()));
        page.save_to_file(&path).unwrap();
        let loaded = PageState::load_from_file(&path, &ctx).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.canvas.objects.len(), 1);
        if let CanvasObject::Stroke(stroke) = &loaded.canvas.objects[0] {
            assert_eq!(stroke.shape, Some(CanvasShapeType::Rectangle));
        } else {
            panic!("Expected Stroke");
        }
        assert_eq!(loaded.history.undo_stack.len(), 1);
        assert!(matches!(
            loaded.history.undo_stack[0],
            HistoryCommand::ReplaceObjects { .. }
        ));
        assert_eq!(loaded.view_offset, egui::Vec2::new(1.0, 2.0));
        assert_eq!(loaded.view_zoom, 1.5);
    }

    fn make_valid_flat() -> PageStateFlat {
        PageStateFlat {
            canvas: CanvasStateFlat {
                objects: vec![CanvasObjectFlat::Stroke(StrokeFlat {
                    points: vec![[0.0, 0.0], [1.0, 1.0]],
                    width: StrokeWidthFlat::Fixed(3.0),
                    color: [255, 255, 255, 255],
                    base_width: 3.0,
                    shape: None,
                })],
            },
            history: HistoryFlat {
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
            },
            view_offset: [0.0, 0.0],
            view_zoom: 1.0,
        }
    }

    #[test]
    fn test_validate_rejects_empty_stroke_points() {
        let mut flat = make_valid_flat();
        if let CanvasObjectFlat::Stroke(stroke) = &mut flat.canvas.objects[0] {
            stroke.points.clear();
        }
        assert!(flat.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_dynamic_width_mismatch() {
        let mut flat = make_valid_flat();
        if let CanvasObjectFlat::Stroke(stroke) = &mut flat.canvas.objects[0] {
            stroke.width = StrokeWidthFlat::Dynamic(vec![1.0]);
        }
        assert!(flat.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_bad_image_data_size() {
        let mut flat = make_valid_flat();
        flat.canvas.objects[0] = CanvasObjectFlat::Image(ImageFlat {
            pos: [0.0, 0.0],
            size: [10.0, 10.0],
            aspect_ratio: 1.0,
            image_data: vec![0u8; 3], // 1x1 RGBA needs 4 bytes
            image_size: [1, 1],
        });
        assert!(flat.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_non_finite_zoom() {
        let mut flat = make_valid_flat();
        flat.view_zoom = f32::NAN;
        assert!(flat.validate().is_err());
    }

    #[test]
    fn test_validate_accepts_valid_flat() {
        assert!(make_valid_flat().validate().is_ok());
    }
}
