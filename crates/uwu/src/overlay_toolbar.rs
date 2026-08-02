use std::sync::Arc;

use crate::app::App;
use crate::render::EguiRenderer;
use crate::state::AppCommand;
use crate::ui;
use crate::utils::ui::apply_theme_mode_and_canvas_color;
use egui_wgpu::{ScreenDescriptor, wgpu};
use wgpu::CurrentSurfaceTexture;
use winit::dpi::{LogicalPosition, LogicalSize, Position};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

pub struct OverlayToolbar {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    pub egui_renderer: EguiRenderer,
}

/// Bottom-center position (in logical coordinates) on the monitor that the
/// main window currently occupies. Falls back to the primary monitor, then to
/// the first available monitor.
fn toolbar_bottom_center_pos(
    main_window: &Window,
    target_w: f64,
    target_h: f64,
) -> Option<LogicalPosition<f64>> {
    let monitor = main_window
        .current_monitor()
        .or_else(|| main_window.primary_monitor())
        .or_else(|| main_window.available_monitors().next());
    let monitor = monitor?;
    let monitor_size = monitor.size();
    let scale = monitor.scale_factor();
    let monitor_w = monitor_size.width as f64 / scale;
    let monitor_h = monitor_size.height as f64 / scale;
    let x = (monitor_w - target_w) / 2.0;
    let y = (monitor_h - target_h - 40.0).max(0.0);
    Some(LogicalPosition::new(x, y))
}

impl App {
    pub fn create_toolbar_window(&mut self, event_loop: &ActiveEventLoop) {
        let window_size = LogicalSize::new(1000.0, 300.0);

        let mut attrs = Window::default_attributes()
            .with_title("uwu")
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_transparent(true)
            .with_inner_size(window_size)
            .with_resizable(true)
            .with_decorations(false);

        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_skip_taskbar(true);
        }

        if let Some(main_window) = self.window.as_ref()
            && let Some(pos) =
                toolbar_bottom_center_pos(main_window, window_size.width, window_size.height)
        {
            attrs = attrs.with_position(Position::Logical(pos));
        }

        let window = event_loop.create_window(attrs).unwrap();

        let window = Arc::new(window);

        #[cfg(windows)]
        unsafe {
            if let Some(hwnd) = crate::utils::windows::winit_window_to_hwnd(&window) {
                if let Err(err) = crate::utils::windows::enable_premultiplied_alpha(hwnd) {
                    eprintln!(
                        "
error: failed to enable premultiplied alpha for toolbar window: {:?}
       overlay mode might not work or app might crash",
                        err
                    );
                }
            }
        };

        let render_state = self.render_state.as_ref().unwrap();

        let surface = self
            .gpu_instance
            .create_surface(window.clone())
            .expect("failed to create toolbar surface");

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
        );

        let ctx = egui_renderer.context().clone();
        apply_theme_mode_and_canvas_color(
            &ctx,
            self.state.persistent.theme_mode,
            self.state.persistent.canvas_color,
        );
        self.state.auxiliary_ctx = Some(ctx);

        self.toolbar_window = Some(OverlayToolbar {
            window,
            surface,
            surface_config,
            egui_renderer,
        });
    }

    pub fn close_helper_window(&mut self) {
        if let Some(helper) = &self.toolbar_window {
            helper.window.set_visible(false);
        }
        if self.state.is_overlay_mode {
            self.state
                .command_queue
                .push(AppCommand::UpdateCursorHittest);
        }
        self.window.as_ref().unwrap().request_redraw();
    }

    fn destroy_toolbar_window(&mut self) {
        self.state.auxiliary_ctx = None;
        self.toolbar_window = None;
    }

    pub fn handle_helper_window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => {
                self.handle_helper_redraw();
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::CloseRequested => {
                self.close_helper_window();
            }
            WindowEvent::Resized(new_size) if new_size.width > 0 && new_size.height > 0 => {
                if let Some(helper) = &mut self.toolbar_window {
                    helper.surface_config.width = new_size.width;
                    helper.surface_config.height = new_size.height;
                    if let Some(rs) = &self.render_state {
                        helper.surface.configure(&rs.device, &helper.surface_config);
                    }
                }
            }
            other => {
                if let Some(helper) = &mut self.toolbar_window {
                    let needs_repaint = helper.egui_renderer.handle_input(&helper.window, &other);
                    if needs_repaint {
                        helper.window.request_redraw();
                    }
                }
            }
        }
    }

    fn handle_helper_redraw(&mut self) {
        let toolbar = self.toolbar_window.as_mut().unwrap();
        let render_state = self.render_state.as_ref().unwrap();

        let surface_texture = match toolbar.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(s) => s,
            CurrentSurfaceTexture::Suboptimal(s) => {
                println!("warning: toolbar wgpu surface suboptimal");
                s
            }
            val => {
                println!("warning: toolbar wgpu surface {:?}", val);
                return;
            }
        };

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        toolbar.egui_renderer.begin_frame(&toolbar.window);

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [toolbar.surface_config.width, toolbar.surface_config.height],
            pixels_per_point: toolbar.egui_renderer.context().pixels_per_point(),
        };

        let ctx = toolbar.egui_renderer.context().clone();

        let id = egui::Id::new((ctx.viewport_id(), "helper_central_panel"));
        let mut panel_ui = egui::Ui::new(
            ctx.clone(),
            id,
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(ctx.content_rect()),
        );
        panel_ui.set_clip_rect(ctx.content_rect());

        let mut toolbar_rect = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::TRANSPARENT))
            .show_inside(&mut panel_ui, |ui| {
                toolbar_rect = ui::ui_toolbar(
                    &mut self.state,
                    ui.ctx(),
                    self.window.as_ref().unwrap(),
                    true,
                );
            });

        if let Some(rect) = toolbar_rect {
            let scale_factor = toolbar.window.scale_factor();
            let toolbar_size = rect.size();
            let padding = 10.0;
            let target_w = (toolbar_size.x + padding * 2.0) as f64;
            let target_h = (toolbar_size.y + padding * 2.0) as f64;

            let current_size: LogicalSize<f64> =
                toolbar.window.inner_size().to_logical(scale_factor);
            if (current_size.width - target_w).abs() > 1.0
                || (current_size.height - target_h).abs() > 1.0
            {
                let _ = toolbar
                    .window
                    .request_inner_size(LogicalSize::new(target_w, target_h));
            }
            // Keep the toolbar pinned to the bottom-center of the main
            // window's monitor (the helper window has no decorations, so it
            // cannot be moved by the user).
            if let Some(main_window) = self.window.as_ref()
                && let Some(pos) = toolbar_bottom_center_pos(main_window, target_w, target_h)
            {
                toolbar.window.set_outer_position(Position::Logical(pos));
            }
        }

        let mut encoder = render_state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        toolbar.egui_renderer.end_frame_and_draw(
            &render_state.device,
            &render_state.queue,
            &mut encoder,
            &toolbar.window,
            &surface_view,
            screen_descriptor,
        );

        render_state.queue.submit(Some(encoder.finish()));
        surface_texture.present();
    }

    pub fn manage_overlay_toolbar(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_overlay_mode {
            if let Some(helper) = &self.toolbar_window {
                helper.window.set_visible(true);
            } else {
                self.create_toolbar_window(event_loop);
            }
        } else {
            if self.toolbar_window.is_some() {
                self.destroy_toolbar_window();
            }
        }
    }

    pub fn request_toolbar_repaint_if_needed(&self) {
        if let Some(helper) = &self.toolbar_window
            && helper.egui_renderer.context().has_requested_repaint()
        {
            helper.window.request_redraw();
        }
    }

    pub fn is_event_for_toolbar(&self, window_id: WindowId) -> bool {
        self.toolbar_window
            .as_ref()
            .is_some_and(|h| h.window.id() == window_id)
    }
}
