mod assets;

use std::ffi::CStr;
use std::time::Instant;

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use rodio::Decoder;
use rodio::DeviceSinkBuilder;
use rodio::Player;
use std::io::Cursor;

use plugin_api::Plugin;

static RUSTC_VERSION_CSTR: &CStr = {
    unsafe { CStr::from_bytes_with_nul_unchecked(concat!(env!("RUSTC_VERSION"), "\0").as_bytes()) }
};

struct StartupAnimationPlugin {
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

impl Plugin for StartupAnimationPlugin {
    fn id(&self) -> &'static str {
        "startup_animation_plugin"
    }
    fn name(&self) -> &'static str {
        "启动动画插件"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn init(&mut self) {
        println!("[startup animation plugin] plugin loaded!")
    }

    fn uninit(&mut self) {
        println!("[startup animation plugin] plugin unloaded!")
    }

    fn ui(&mut self, ctx: &egui::Context) {
        if !self.finished {
            self.update(ctx);
            self.draw_fullscreen(ctx);
            ctx.request_repaint(); // ensure smooth playback
        }
    }
}

impl StartupAnimationPlugin {
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
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn plugin_rustc_version() -> *const std::ffi::c_char {
    RUSTC_VERSION_CSTR.as_ptr()
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub unsafe extern "C" fn plugin_create() -> *mut dyn Plugin {
    let plugin: Box<dyn Plugin> = Box::new(StartupAnimationPlugin::new(
        30.0,
        assets::STARTUP_FRAMES,
        assets::STARTUP_AUDIO,
    ));
    Box::into_raw(plugin)
}
