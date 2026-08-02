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
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct ObjectTransformFlat {
    pub pos: [f32; 2],
    pub size: [f32; 2],
}

// ===== Helper: convert individual CanvasObject ↔ CanvasObjectFlat =====

fn canvas_object_to_flat(obj: &CanvasObject) -> Option<CanvasObjectFlat> {
    match obj {
        CanvasObject::Stroke(s) => Some(CanvasObjectFlat::Stroke(StrokeFlat {
            points: s.points.iter().map(|p| [p.x, p.y]).collect(),
            width: match &s.width {
                StrokeWidth::Fixed(w) => StrokeWidthFlat::Fixed(*w),
                StrokeWidth::Dynamic(v) => StrokeWidthFlat::Dynamic(v.clone()),
            },
            color: [s.color.r(), s.color.g(), s.color.b(), s.color.a()],
            base_width: s.base_width,
        })),
        CanvasObject::Text(t) => Some(CanvasObjectFlat::Text(TextFlat {
            text: t.text.clone(),
            pos: [t.pos.x, t.pos.y],
            color: [t.color.r(), t.color.g(), t.color.b(), t.color.a()],
            font_size: t.font_size,
        })),
        CanvasObject::Shape(s) => Some(CanvasObjectFlat::Shape(ShapeFlat {
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
        })),
        CanvasObject::Image(img) => {
            let data: Vec<u8> = img.image_data.to_vec();
            Some(CanvasObjectFlat::Image(ImageFlat {
                pos: [img.pos.x, img.pos.y],
                size: [img.size.x, img.size.y],
                aspect_ratio: img.aspect_ratio,
                image_data: data,
                image_size: img.image_size,
            }))
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
            shape: None,
        }),
        CanvasObjectFlat::Text(t) => CanvasObject::Text(CanvasText {
            text: t.text,
            pos: Pos2::new(t.pos[0], t.pos[1]),
            color: Color32::from_rgba_unmultiplied(t.color[0], t.color[1], t.color[2], t.color[3]),
            font_size: t.font_size,
            cached_size: None,
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
            let color_image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba_data);
            let texture =
                ctx.load_texture("loaded_image", color_image, egui::TextureOptions::LINEAR);
            let image_data: std::sync::Arc<[u8]> = rgba_data.into();
            CanvasObject::Image(CanvasImage {
                texture,
                pos: Pos2::new(img.pos[0], img.pos[1]),
                size: egui::Vec2::new(img.size[0], img.size[1]),
                aspect_ratio: img.aspect_ratio,
                marked_for_deletion: false,
                image_data,
                image_size: img.image_size,
            })
        }
    }
}

// ===== Conversions: runtime → flat =====

impl From<&CanvasState> for CanvasStateFlat {
    fn from(state: &CanvasState) -> Self {
        CanvasStateFlat {
            objects: state
                .objects
                .iter()
                .filter_map(canvas_object_to_flat)
                .collect(),
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

fn history_command_to_flat(cmd: &HistoryCommand) -> Option<HistoryCommandFlat> {
    match cmd {
        HistoryCommand::AddObject { index, object } => {
            canvas_object_to_flat(object).map(|obj| HistoryCommandFlat::AddObject {
                index: *index as u32,
                object: obj,
            })
        }
        HistoryCommand::RemoveObject { index, object } => {
            canvas_object_to_flat(object).map(|obj| HistoryCommandFlat::RemoveObject {
                index: *index as u32,
                object: obj,
            })
        }
        HistoryCommand::ClearObjects { objects } => {
            let flat_objects: Vec<CanvasObjectFlat> =
                objects.iter().filter_map(canvas_object_to_flat).collect();
            Some(HistoryCommandFlat::ClearObjects {
                objects: flat_objects,
            })
        }
        HistoryCommand::MoveObject {
            index,
            old_position,
            new_position,
        } => Some(HistoryCommandFlat::MoveObject {
            index: *index as u32,
            old_position: [old_position.x, old_position.y],
            new_position: [new_position.x, new_position.y],
        }),
        HistoryCommand::TransformObject {
            index,
            old_transform,
            new_transform,
        } => Some(HistoryCommandFlat::TransformObject {
            index: *index as u32,
            old_transform: ObjectTransformFlat::from(old_transform),
            new_transform: ObjectTransformFlat::from(new_transform),
        }),
        // FIXME: batch commands are ignored
        HistoryCommand::BatchCommand { .. } => None,
        // FIXME: replaced in a later commit with a proper flat variant
        HistoryCommand::ReplaceObjects { .. } => None,
    }
}

impl From<&History> for HistoryFlat {
    fn from(history: &History) -> Self {
        HistoryFlat {
            undo_stack: history
                .undo_stack
                .iter()
                .filter_map(history_command_to_flat)
                .collect(),
            redo_stack: history
                .redo_stack
                .iter()
                .filter_map(history_command_to_flat)
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

        std::fs::write(path, out)?;
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

        let flat = canvas_object_to_flat(&shape).expect("Failed to convert to flat");

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
            cached_size: None,
        });

        let flat = canvas_object_to_flat(&text).expect("Failed to convert to flat");

        if let CanvasObjectFlat::Text(flat_text) = flat {
            assert_eq!(flat_text.text, "Hello");
            assert_eq!(flat_text.pos, [1.0, 2.0]);
            assert_eq!(flat_text.font_size, 14.0);
            assert_eq!(flat_text.color, [0, 255, 0, 255]);
        } else {
            panic!("Expected TextFlat");
        }
    }
}
