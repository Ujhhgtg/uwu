use rkyv::{Archive, Deserialize, Serialize};

use super::{
    CanvasImage, CanvasObject, CanvasShape, CanvasShapeType, CanvasState, CanvasStroke, CanvasText,
    Color32, History, HistoryCommand, ObjectTransform, PageState, Pos2, StrokeWidth,
};

// ===== Flat data types for rkyv serialization =====

#[derive(rkyv::Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct PageStateFlat {
    pub canvas: CanvasStateFlat,
    pub history: HistoryFlat,
    pub view_offset: [f32; 2],
    pub view_zoom: f32,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct CanvasStateFlat {
    pub objects: Vec<CanvasObjectFlat>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub enum CanvasObjectFlat {
    Stroke(StrokeFlat),
    Text(TextFlat),
    Shape(ShapeFlat),
    Image(ImageFlat),
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct StrokeFlat {
    pub points: Vec<[f32; 2]>,
    pub width: StrokeWidthFlat,
    pub color: [u8; 4],
    pub base_width: f32,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub enum StrokeWidthFlat {
    Fixed(f32),
    Dynamic(Vec<f32>),
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct TextFlat {
    pub text: String,
    pub pos: [f32; 2],
    pub color: [u8; 4],
    pub font_size: f32,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct ShapeFlat {
    pub shape_type: ShapeTypeFlat,
    pub pos: [f32; 2],
    pub size: f32,
    pub color: [u8; 4],
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub enum ShapeTypeFlat {
    Line,
    Arrow,
    Rectangle,
    Triangle,
    Circle,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct ImageFlat {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub aspect_ratio: f32,
    pub image_data: Vec<u8>,
    pub image_size: [u32; 2],
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
pub struct HistoryFlat {
    pub undo_stack: Vec<HistoryCommandFlat>,
    pub redo_stack: Vec<HistoryCommandFlat>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
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

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(bytecheck())]
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

fn archived_object_to_canvas_object(
    obj: &ArchivedCanvasObjectFlat,
    ctx: &egui::Context,
) -> CanvasObject {
    match obj {
        ArchivedCanvasObjectFlat::Stroke(s) => CanvasObject::Stroke(CanvasStroke {
            points: s
                .points
                .iter()
                .map(|p| Pos2::new(p[0].into(), p[1].into()))
                .collect(),
            width: match &s.width {
                ArchivedStrokeWidthFlat::Fixed(w) => StrokeWidth::Fixed((*w).into()),
                ArchivedStrokeWidthFlat::Dynamic(v) => {
                    StrokeWidth::Dynamic(v.iter().map(|&x| x.into()).collect())
                }
            },
            color: Color32::from_rgba_unmultiplied(s.color[0], s.color[1], s.color[2], s.color[3]),
            base_width: s.base_width.into(),
            shape: None,
        }),
        ArchivedCanvasObjectFlat::Text(t) => CanvasObject::Text(CanvasText {
            text: t.text.as_str().to_string(),
            pos: Pos2::new(t.pos[0].into(), t.pos[1].into()),
            color: Color32::from_rgba_unmultiplied(t.color[0], t.color[1], t.color[2], t.color[3]),
            font_size: t.font_size.into(),
            cached_size: None,
        }),
        ArchivedCanvasObjectFlat::Shape(s) => CanvasObject::Shape(CanvasShape {
            shape_type: match s.shape_type {
                ArchivedShapeTypeFlat::Line => CanvasShapeType::Line,
                ArchivedShapeTypeFlat::Arrow => CanvasShapeType::Arrow,
                ArchivedShapeTypeFlat::Rectangle => CanvasShapeType::Rectangle,
                ArchivedShapeTypeFlat::Triangle => CanvasShapeType::Triangle,
                ArchivedShapeTypeFlat::Circle => CanvasShapeType::Circle,
            },
            pos: Pos2::new(s.pos[0].into(), s.pos[1].into()),
            size: s.size.into(),
            color: Color32::from_rgba_unmultiplied(s.color[0], s.color[1], s.color[2], s.color[3]),
        }),
        ArchivedCanvasObjectFlat::Image(img) => {
            let width = u32::from(img.image_size[0]) as usize;
            let height = u32::from(img.image_size[1]) as usize;
            let rgba_data: Vec<u8> = img.image_data.iter().copied().collect();
            let color_image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba_data);
            let texture =
                ctx.load_texture("loaded_image", color_image, egui::TextureOptions::LINEAR);
            let image_data: std::sync::Arc<[u8]> = rgba_data.into();
            CanvasObject::Image(CanvasImage {
                texture,
                pos: Pos2::new(img.pos[0].into(), img.pos[1].into()),
                size: egui::Vec2::new(img.size[0].into(), img.size[1].into()),
                aspect_ratio: img.aspect_ratio.into(),
                marked_for_deletion: false,
                image_data,
                image_size: [u32::from(img.image_size[0]), u32::from(img.image_size[1])],
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

// ===== Conversions: flat (archived) → runtime =====

impl PageState {
    /// Loads a PageState from an rkyv-archived file, using the egui context
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
        let archived = rkyv::access::<ArchivedPageStateFlat, rkyv::rancor::Error>(payload)
            .map_err(|e| format!("rkyv error: {e}"))?;

        Ok(Self::from_archived(archived, ctx))
    }

    fn from_archived(archived: &ArchivedPageStateFlat, ctx: &egui::Context) -> Self {
        PageState {
            canvas: CanvasState {
                objects: archived
                    .canvas
                    .objects
                    .iter()
                    .map(|obj| archived_object_to_canvas_object(obj, ctx))
                    .collect(),
            },
            history: History {
                undo_stack: archived
                    .history
                    .undo_stack
                    .iter()
                    .map(|cmd| archived_history_command_to_runtime(cmd, ctx))
                    .collect(),
                redo_stack: archived
                    .history
                    .redo_stack
                    .iter()
                    .map(|cmd| archived_history_command_to_runtime(cmd, ctx))
                    .collect(),
                max_history_size: 50,
            },
            view_offset: egui::Vec2::new(
                archived.view_offset[0].into(),
                archived.view_offset[1].into(),
            ),
            view_zoom: archived.view_zoom.into(),
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

    /// Saves the page state to a file using rkyv binary format.
    pub fn save_to_file(
        &self,
        path: &std::path::PathBuf,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let flat = PageStateFlat::from(self);
        let payload =
            rkyv::to_bytes::<rkyv::rancor::Error>(&flat).map_err(|e| format!("rkyv error: {e}"))?;

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

fn archived_history_command_to_runtime(
    cmd: &ArchivedHistoryCommandFlat,
    ctx: &egui::Context,
) -> HistoryCommand {
    match cmd {
        ArchivedHistoryCommandFlat::AddObject { index, object } => HistoryCommand::AddObject {
            index: u32::from(*index) as usize,
            object: archived_object_to_canvas_object(object, ctx),
        },
        ArchivedHistoryCommandFlat::RemoveObject { index, object } => {
            HistoryCommand::RemoveObject {
                index: u32::from(*index) as usize,
                object: archived_object_to_canvas_object(object, ctx),
            }
        }
        ArchivedHistoryCommandFlat::ClearObjects { objects } => HistoryCommand::ClearObjects {
            objects: objects
                .iter()
                .map(|obj| archived_object_to_canvas_object(obj, ctx))
                .collect(),
        },
        ArchivedHistoryCommandFlat::MoveObject {
            index,
            old_position,
            new_position,
        } => HistoryCommand::MoveObject {
            index: u32::from(*index) as usize,
            old_position: egui::Vec2::new(old_position[0].into(), old_position[1].into()),
            new_position: egui::Vec2::new(new_position[0].into(), new_position[1].into()),
        },
        ArchivedHistoryCommandFlat::TransformObject {
            index,
            old_transform,
            new_transform,
        } => HistoryCommand::TransformObject {
            index: u32::from(*index) as usize,
            old_transform: ObjectTransform {
                pos: Pos2::new(old_transform.pos[0].into(), old_transform.pos[1].into()),
                size: egui::Vec2::new(old_transform.size[0].into(), old_transform.size[1].into()),
            },
            new_transform: ObjectTransform {
                pos: Pos2::new(new_transform.pos[0].into(), new_transform.pos[1].into()),
                size: egui::Vec2::new(new_transform.size[0].into(), new_transform.size[1].into()),
            },
        },
    }
}
