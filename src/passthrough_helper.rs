use std::sync::Arc;

use crate::app::App;
use crate::render::EguiRenderer;
use crate::state::AppCommand;
use crate::state::CanvasTool;
use crate::ui;
use crate::utils::ui::apply_theme_mode_and_canvas_color;
use egui_wgpu::{ScreenDescriptor, wgpu};
use wgpu::CurrentSurfaceTexture;
use winit::dpi::{LogicalPosition, LogicalSize, Position};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

pub struct PassthroughHelper {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    pub egui_renderer: EguiRenderer,
}

impl App {
    pub fn create_helper_window(&mut self, event_loop: &ActiveEventLoop) {
        let window_size = LogicalSize::new(420.0, 60.0);

        let mut attrs = Window::default_attributes()
            .with_title("uwu")
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_transparent(false)
            .with_inner_size(window_size)
            .with_resizable(false)
            .with_decorations(false);

        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_skip_taskbar(true);
        }

        if let Some(monitor) = event_loop.primary_monitor() {
            let monitor_size = monitor.size();
            let scale = monitor.scale_factor();
            let monitor_w = monitor_size.width as f64 / scale;
            let monitor_h = monitor_size.height as f64 / scale;
            let x = (monitor_w - window_size.width) / 2.0;
            let y = monitor_h - window_size.height - 40.0;
            attrs = attrs.with_position(Position::Logical(LogicalPosition::new(x, y)));
        }

        let window = event_loop.create_window(attrs).unwrap();

        let window = Arc::new(window);

        let render_state = self.render_state.as_ref().unwrap();

        let surface = self
            .gpu_instance
            .create_surface(window.clone())
            .expect("failed to create helper surface");

        let size = window.inner_size();
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };

        surface.configure(&render_state.device, &surface_config);

        let egui_renderer = EguiRenderer::new(
            &render_state.device,
            surface_config.format,
            None,
            1,
            &window,
            window.scale_factor() as f32,
        );

        let ctx = egui_renderer.context().clone();
        apply_theme_mode_and_canvas_color(
            &ctx,
            self.state.persistent.theme_mode,
            self.state.persistent.canvas_color,
        );

        self.helper_window = Some(PassthroughHelper {
            window,
            surface,
            surface_config,
            egui_renderer,
        });
    }

    pub fn close_helper_window(&mut self) {
        if let Some(helper) = &self.helper_window {
            helper.window.set_visible(false);
        }
        if self.state.is_overlay_mode {
            self.state
                .command_queue
                .push(AppCommand::UpdateCursorHittest);
        }
        self.window.as_ref().unwrap().request_redraw();
    }

    fn destroy_helper_window(&mut self) {
        self.helper_window = None;
    }

    pub fn handle_helper_window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => {
                if self.handle_helper_redraw() {
                    self.close_helper_window();
                    self.window.as_ref().unwrap().request_redraw();
                }
            }
            WindowEvent::CloseRequested => {
                self.close_helper_window();
            }
            WindowEvent::Resized(new_size) if new_size.width > 0 && new_size.height > 0 => {
                if let Some(helper) = &mut self.helper_window {
                    helper.surface_config.width = new_size.width;
                    helper.surface_config.height = new_size.height;
                    if let Some(rs) = &self.render_state {
                        helper.surface.configure(&rs.device, &helper.surface_config);
                    }
                }
            }
            other => {
                if let Some(helper) = &mut self.helper_window {
                    let needs_repaint = helper.egui_renderer.handle_input(&helper.window, &other);
                    if needs_repaint {
                        helper.window.request_redraw();
                    }
                }
            }
        }
    }

    fn handle_helper_redraw(&mut self) -> bool {
        let helper = self.helper_window.as_mut().unwrap();
        let render_state = self.render_state.as_ref().unwrap();

        let surface_texture = match helper.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(s) => s,
            CurrentSurfaceTexture::Suboptimal(s) => {
                println!("warning: helper wgpu surface suboptimal");
                s
            }
            val => {
                println!("warning: helper wgpu surface {:?}", val);
                return false;
            }
        };

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [helper.surface_config.width, helper.surface_config.height],
            pixels_per_point: helper.window.scale_factor() as f32,
        };

        helper.egui_renderer.begin_frame(&helper.window);

        let ctx = helper.egui_renderer.context().clone();

        let clicked_return = ui::ui_passthrough_helper(&mut self.state, &ctx);

        let mut encoder = render_state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        helper.egui_renderer.end_frame_and_draw(
            &render_state.device,
            &render_state.queue,
            &mut encoder,
            &helper.window,
            &surface_view,
            screen_descriptor,
        );

        render_state.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        clicked_return
    }

    pub fn manage_passthrough_helper(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_overlay_mode && self.state.current_tool == CanvasTool::Passthrough {
            if let Some(helper) = &self.helper_window {
                helper.window.set_visible(true);
            } else {
                self.create_helper_window(event_loop);
            }
        } else {
            let destroy = !self.state.is_overlay_mode;
            let has_helper = self.helper_window.is_some();
            if has_helper {
                if destroy {
                    self.destroy_helper_window();
                } else {
                    self.helper_window
                        .as_ref()
                        .unwrap()
                        .window
                        .set_visible(false);
                }
            }
        }
    }

    pub fn request_helper_repaint_if_needed(&self) {
        if let Some(helper) = &self.helper_window
            && helper.egui_renderer.context().has_requested_repaint()
        {
            helper.window.request_redraw();
        }
    }

    pub fn is_event_for_helper(&self, window_id: WindowId) -> bool {
        self.helper_window
            .as_ref()
            .is_some_and(|h| h.window.id() == window_id)
    }
}
