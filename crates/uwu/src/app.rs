use crate::assets::ICON;
use crate::passthrough_helper::PassthroughHelper;
use crate::render::RenderState;
use crate::single_instance;
use crate::state::{
    AppCommand, AppState, CanvasObject, CanvasObjectOps, CanvasTool, HistoryCommand, InsertTab,
    MarqueeMatchMode, PointerInteraction, PointerState,
};
use crate::ui;
use crate::utils;
use crate::utils::plugins::{PluginAlreadyLoaded, load_plugin_from_path};
use crate::utils::stroke::{brush_stroke_add_point, brush_stroke_end, brush_stroke_start};
use crate::utils::ui::{apply_theme_mode_and_canvas_color, apply_window_mode};
use core::f32;
use egui::{Pos2, Vec2};
use egui_wgpu::{ScreenDescriptor, wgpu};
use image::GenericImageView;
use std::sync::Arc;
use wgpu::{
    BackendOptions, CurrentSurfaceTexture, InstanceDescriptor, TexelCopyBufferInfo,
    TexelCopyBufferLayout,
};
use wgpu::{InstanceFlags, TexelCopyTextureInfo};
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, Touch, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

pub struct App {
    pub gpu_instance: wgpu::Instance,
    pub render_state: Option<RenderState>,
    pub window: Option<Arc<Window>>,
    pub state: AppState,
    pub helper_window: Option<PassthroughHelper>,
}

impl App {
    pub fn new() -> Self {
        let mut state = AppState::default();
        let gpu_instance = wgpu::Instance::new(InstanceDescriptor {
            backends: state.persistent.graphics_api.to_backends(),
            flags: InstanceFlags::empty(),
            memory_budget_thresholds: Default::default(),
            backend_options: {
                let mut options = BackendOptions::default();
                options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual; // enable DirectComposition for transparency
                options
            },
            display: None,
        });

        if !state.persistent.show_welcome_window_on_start {
            state.show_welcome_window = false
        }

        // Auto-load plugins from persistent settings
        for path in state.persistent.plugin_paths.clone() {
            let existing: Vec<&str> = state.plugins.iter().map(|p| p.id.as_str()).collect();
            match load_plugin_from_path(path, &existing) {
                Ok(loaded) => state.plugins.push(loaded),
                Err(e) => eprintln!("warning: failed to auto-load plugin: {}", e),
            }
        }

        Self {
            gpu_instance,
            render_state: None,
            window: None,
            state,
            helper_window: None,
        }
    }

    pub async fn create_window(&mut self, event_loop: &ActiveEventLoop) {
        // icon
        let icon = image::load_from_memory(ICON).expect("invalid icon data");
        let rgba = icon.to_rgba8().to_vec();
        let (width, height) = icon.dimensions();
        let winit_icon =
            Some(winit::window::Icon::from_rgba(rgba, width, height).expect("invalid icon data"));

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("uwu")
                        .with_transparent(true)
                        .with_window_icon({
                            #[cfg(windows)]
                            {
                                winit_icon.clone()
                            }
                            #[cfg(not(windows))]
                            {
                                winit_icon
                            }
                        }),
                )
                .unwrap(),
        );

        #[cfg(windows)]
        {
            use winit::platform::windows::WindowExtWindows;
            window.set_taskbar_icon(winit_icon);
        }

        // prepare exclusive fullscreen video modes
        let monitor = window
            .current_monitor()
            .or_else(|| window.primary_monitor())
            .or_else(|| window.available_monitors().next());
        if let Some(monitor) = monitor {
            self.state.fullscreen_video_modes = monitor.video_modes().collect();
        } else {
            eprintln!(
                "warning: failed to get monitor, exclusive fullscreen mode will be unavailable"
            )
        }

        // window mode
        apply_window_mode(&mut self.state, &window);

        #[cfg(windows)]
        unsafe {
            if let Err(err) = utils::windows::enable_premultiplied_alpha(
                utils::windows::winit_window_to_hwnd(&window).unwrap(),
            ) {
                eprintln!(
                    "
error: failed to enable premultiplied alpha for window: {:?}
       passthrough mode might not work or app might crash",
                    err
                );
            }
        };

        // prepare renderer
        let size = window.inner_size();
        let initial_width = size.width;
        let initial_height = size.height;

        let surface = self
            .gpu_instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        let state = RenderState::new(
            &self.gpu_instance,
            surface,
            &window,
            initial_width,
            initial_height,
            self.state.persistent.optimization_policy,
            self.state.persistent.present_mode,
        )
        .await;

        self.state.active_backend = Some(state.device.adapter_info().backend);

        let ctx = state.egui_renderer.context();

        // colors
        apply_theme_mode_and_canvas_color(
            ctx,
            self.state.persistent.theme_mode,
            self.state.persistent.canvas_color,
        );

        // first draw
        window.request_redraw();

        self.window.get_or_insert(window);
        self.render_state.get_or_insert(state);
    }

    fn exit(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(err) = self.state.persistent.save_to_file() {
            eprintln!("failed to save settings: {}", err);
        }
        event_loop.exit();
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        self.render_state
            .as_mut()
            .unwrap()
            .resize_surface(width, height);
    }

    #[cfg_attr(feature = "profiling", profiling::function)]
    fn handle_redraw(&mut self) {
        #[cfg(feature = "profiling")]
        profiling::scope!("handle_redraw::setup");

        let render_state = unsafe { self.render_state.as_mut().unwrap_unchecked() };
        let window = unsafe { self.window.as_ref().unwrap_unchecked() };

        // process deferred commands
        for cmd in self.state.command_queue.drain(..) {
            match cmd {
                AppCommand::SetPresentMode(mode) => {
                    render_state.set_present_mode(mode);
                }
                AppCommand::UpdateCursorHittest => {
                    let passthrough = self.state.is_overlay_mode
                        && self.state.current_tool == CanvasTool::Passthrough;
                    let _ = window.set_cursor_hittest(!passthrough);
                }
                AppCommand::LoadPlugin(path) => {
                    let plugin_path = path.clone();
                    let existing: Vec<&str> =
                        self.state.plugins.iter().map(|p| p.id.as_str()).collect();
                    match load_plugin_from_path(path, &existing) {
                        Ok(loaded) => {
                            let name = loaded.name.clone();
                            let loaded_path = loaded.path.clone();
                            // Persist the plugin path if not already tracked
                            if !self.state.persistent.plugin_paths.contains(&loaded_path) {
                                self.state.persistent.plugin_paths.push(loaded_path);
                            }
                            self.state.plugins.push(loaded);
                            self.state.toasts.success(format!("插件加载成功: {}", name));
                        }
                        Err(e) => {
                            if let Some(dup) = e.downcast_ref::<PluginAlreadyLoaded>() {
                                // Re-add to persistent paths even if already loaded
                                if !self.state.persistent.plugin_paths.contains(&plugin_path) {
                                    self.state.persistent.plugin_paths.push(plugin_path);
                                }
                                self.state
                                    .toasts
                                    .error(format!("插件 '{}' 已经加载, 请勿重复加载!", dup.id));
                            } else {
                                self.state.toasts.error(format!("插件加载失败: {}", e));
                            }
                        }
                    }
                } // TODO: exiting after doing this triggers a SIGSEGV on linux
                  // AppCommand::UnloadAllPlugins => {
                  // self.state.plugins.clear();
                  // }
            }
        }

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [
                render_state.surface_config.width,
                render_state.surface_config.height,
            ],
            pixels_per_point: window.scale_factor() as f32 * render_state.scale_factor,
        };

        let surface_texture = render_state.surface.get_current_texture();

        let surface_texture = match surface_texture {
            CurrentSurfaceTexture::Success(surface) => surface,
            CurrentSurfaceTexture::Suboptimal(surface) => {
                println!("warning: wgpu surface suboptimal");
                surface
            }
            val => {
                println!("warning: wgpu surface {:?}", val);
                return;
            }
        };

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = render_state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        render_state.egui_renderer.begin_frame(window);

        // access this value in next redraw before ui to ensure that all ui has become invisible
        let screenshot_path = self.state.screenshot_path.clone();

        // fixes a borrow checker error
        let ctx = &(render_state.egui_renderer.context().clone());

        // --- plugin hooks ---
        for loaded in &mut self.state.plugins {
            loaded.plugin.before_ui();
        }

        // --- ui ---
        {
            #[cfg(feature = "profiling")]
            profiling::scope!("handle_redraw::ui");

            if self.state.current_tool != CanvasTool::Passthrough
                && self.state.screenshot_path.is_none()
            {
                self.state.toasts.show(ctx);

                #[cfg(feature = "profiling")]
                puffin_egui::profiler_window(ctx);

                if self.state.show_welcome_window {
                    ui::ui_welcome(&mut self.state, ctx);
                }

                ui::ui_toolbar(&mut self.state, ctx, window);

                ui::ui_pages_nav(&mut self.state, ctx);

                if self.state.show_page_management_window {
                    ui::ui_pages_manager(&mut self.state, ctx);
                }

                // --- plugin hooks ---
                for loaded in &mut self.state.plugins {
                    loaded.plugin.ui(ctx);
                }
            }

            ui::ui_canvas(&mut self.state, ctx);
        };
        // --- end ui

        // egui render pass
        {
            #[cfg(feature = "profiling")]
            profiling::scope!("handle_redraw::render_pass");

            render_state.egui_renderer.end_frame_and_draw(
                &render_state.device,
                &render_state.queue,
                &mut encoder,
                window,
                &surface_view,
                screen_descriptor,
            );
        }

        // submit & present texture
        if let Some(path) = screenshot_path {
            #[cfg(feature = "profiling")]
            profiling::scope!("handle_redraw::screenshot");

            let width = render_state.surface_config.width;
            let height = render_state.surface_config.height;

            let bytes_per_pixel = 4;
            let unpadded_bytes_per_row = width * bytes_per_pixel;

            // wgpu requires 256-byte alignment
            const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(ALIGN) * ALIGN;

            let buffer_size = (padded_bytes_per_row * height) as u64;

            let output_buffer = render_state.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("screenshot buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

            encoder.copy_texture_to_buffer(
                TexelCopyTextureInfo {
                    texture: &surface_texture.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                TexelCopyBufferInfo {
                    buffer: &output_buffer,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );

            render_state.queue.submit(Some(encoder.finish()));

            let buffer_slice = output_buffer.slice(..);

            buffer_slice.map_async(wgpu::MapMode::Read, |_| {});

            // ensure gpu work is done
            let _ = render_state.device.poll(wgpu::wgt::PollType::Wait {
                submission_index: None,
                timeout: None,
            });

            let data = buffer_slice.get_mapped_range();

            let mut pixels = vec![0u8; (width * height * 4) as usize];

            for y in 0..height as usize {
                let src_offset = y * padded_bytes_per_row as usize;
                let dst_offset = y * unpadded_bytes_per_row as usize;

                pixels[dst_offset..dst_offset + unpadded_bytes_per_row as usize].copy_from_slice(
                    &data[src_offset..src_offset + unpadded_bytes_per_row as usize],
                );
            }

            // pixels
            //     .chunks_exact(width as usize * 4)
            //     .collect::<Vec<_>>()
            //     .into_iter()
            //     .rev()
            //     .flatten()
            //     .copied()
            //     .collect::<Vec<u8>>();

            for chunk in pixels.chunks_exact_mut(4) {
                chunk.swap(0, 2); // B ↔ R
            }

            match image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8) {
                Ok(_) => {
                    self.state.toasts.success("成功导出为图片!");
                }
                Err(err) => {
                    self.state.toasts.error(format!("图片导出失败: {}!", err));
                }
            }

            drop(data);
            output_buffer.unmap();

            self.state.screenshot_path = None;
        } else {
            render_state.queue.submit(Some(encoder.finish()));
        }

        {
            #[cfg(feature = "profiling")]
            profiling::scope!("handle_redraw::gc");

            self.state.canvas.objects.retain(|obj| {
                if let CanvasObject::Image(img) = obj {
                    !img.marked_for_deletion
                } else {
                    true
                }
            });
        }

        surface_texture.present();

        if self.state.persistent.show_fps {
            _ = self.state.fps_counter.update();
        }

        #[cfg(feature = "profiling")]
        profiling::finish_frame!();
    }
}

impl ApplicationHandler<()> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        pollster::block_on(self.create_window(event_loop));
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if single_instance::FOCUS_REQUESTED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let window = self.window.as_ref().unwrap();
            window.set_minimized(false);
            window.focus_window();
        }

        self.request_helper_repaint_if_needed();

        // redraw if egui requests repaint
        if self
            .render_state
            .as_ref()
            .unwrap()
            .egui_renderer
            .context()
            .has_requested_repaint()
        {
            self.window.as_ref().unwrap().request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Dispatch to helper window if this event is for it
        if self.is_event_for_helper(window_id) {
            self.handle_helper_window_event(event_loop, event);
            return;
        }

        if self.state.should_quit {
            println!("quit button was pressed; exiting");
            self.exit(event_loop);
            return;
        }

        // redraw only on input
        // don't pass RedrawRequested to egui's input handler,
        // it's not input and would make egui request a repaint, causing an infinite redraw loop
        if self.state.persistent.force_redraw_every_frame
            || !matches!(event, WindowEvent::RedrawRequested)
        {
            let egui_needs_repaint = self
                .render_state
                .as_mut()
                .unwrap()
                .egui_renderer
                .handle_input(self.window.as_ref().unwrap(), &event);

            if self.state.persistent.force_redraw_every_frame || egui_needs_repaint {
                self.window.as_ref().unwrap().request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                self.exit(event_loop);
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: winit::event::ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                self.exit(event_loop);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw();
                self.manage_passthrough_helper(event_loop);
            }
            WindowEvent::Resized(new_size) if new_size.width > 0 && new_size.height > 0 => {
                self.handle_resized(new_size.width, new_size.height);
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::Touch(Touch {
                phase,
                location,
                id,
                ..
            }) => {
                // Convert touch location to logical coordinates (screen space)
                let window = self.window.as_ref().unwrap();
                let scale_factor = window.scale_factor() as f32;
                let screen_pos = Pos2::new(
                    location.x as f32 / scale_factor,
                    location.y as f32 / scale_factor,
                );
                let pos = screen_pos / self.state.view_zoom + self.state.view_offset;

                match phase {
                    TouchPhase::Started => match self.state.current_tool {
                        CanvasTool::View => {
                            self.state.pointers.insert(
                                id,
                                PointerState {
                                    id,
                                    pos,
                                    prev_pos: None,
                                    interaction: PointerInteraction::Panning {
                                        last_pos: screen_pos,
                                    },
                                },
                            );
                            self.state.init_pinch_if_two_panning();
                        }
                        CanvasTool::Brush => {
                            brush_stroke_start(&mut self.state, id, pos);
                        }
                        CanvasTool::Select
                            if !self.state.pointers.values().any(|p| {
                                matches!(
                                    p.interaction,
                                    PointerInteraction::Selecting { .. }
                                        | PointerInteraction::MarqueeSelect { .. }
                                )
                            }) =>
                        {
                            // Hit-test objects (last to first for z-order) to detect drag target
                            let hit_idx = self
                                .state
                                .canvas
                                .objects
                                .iter()
                                .enumerate()
                                .rev()
                                .find(|(_, obj)| obj.bounding_box().contains(pos))
                                .map(|(i, _)| i);

                            if let Some(hit) = hit_idx
                                && (self.state.persistent.click_or_drag_to_single_select
                                    || self.state.is_selected(hit))
                            {
                                // Touch on object: single select and prepare for drag
                                if !self.state.is_selected(hit) {
                                    self.state.clear_selection();
                                    self.state.selected_object_indices.push(hit);
                                }
                                let object = &self.state.canvas.objects[hit];
                                let bbox = object.bounding_box();
                                let handle = utils::get_transform_handle_at_pos(bbox, pos);
                                let transforms = vec![(hit, object.get_transform())];
                                self.state.pointers.insert(
                                    id,
                                    PointerState {
                                        id,
                                        pos,
                                        prev_pos: None,
                                        interaction: PointerInteraction::Selecting {
                                            drag_start: pos,
                                            dragged_handle: handle,
                                            drag_original_transforms: transforms,
                                            drag_accumulated_delta: Vec2::ZERO,
                                        },
                                    },
                                );
                            } else {
                                // Touch on empty space: marquee select
                                self.state.clear_selection();
                                self.state.pointers.insert(
                                    id,
                                    PointerState {
                                        id,
                                        pos,
                                        prev_pos: None,
                                        interaction: PointerInteraction::MarqueeSelect {
                                            drag_start: pos,
                                            points: vec![pos],
                                        },
                                    },
                                );
                            }
                        }
                        CanvasTool::ObjectEraser | CanvasTool::PixelEraser => {
                            self.state.pointers.insert(
                                id,
                                PointerState {
                                    id,
                                    pos,
                                    prev_pos: None,
                                    interaction: PointerInteraction::Erasing,
                                },
                            );
                        }
                        CanvasTool::Insert
                            if self.state.current_insert_tab == InsertTab::Shape
                                && self.state.selected_shape_type.is_some() =>
                        {
                            let shape_type = self.state.selected_shape_type.unwrap();
                            self.state.pointers.insert(
                                id,
                                PointerState {
                                    id,
                                    pos,
                                    prev_pos: None,
                                    interaction: PointerInteraction::ShapeInsert {
                                        start_pos: pos,
                                        shape_type,
                                    },
                                },
                            );
                        }
                        _ => {}
                    },
                    TouchPhase::Moved => match self.state.current_tool {
                        CanvasTool::View => {
                            let pinching = self.state.pinch_state.is_some();
                            if let Some(pointer) = self.state.pointers.get_mut(&id) {
                                if let PointerInteraction::Panning { ref mut last_pos } =
                                    pointer.interaction
                                {
                                    if !pinching {
                                        let delta = screen_pos - *last_pos;
                                        self.state.view_offset -= delta / self.state.view_zoom;
                                    }
                                    *last_pos = screen_pos;
                                }
                                pointer.pos = pos;
                            }
                            if pinching {
                                self.state.apply_pinch_zoom();
                            } else {
                                self.state.init_pinch_if_two_panning();
                            }
                        }
                        CanvasTool::Brush => {
                            brush_stroke_add_point(&mut self.state, id, pos, false);
                        }
                        CanvasTool::Select => {
                            if let Some(pointer) = self.state.pointers.get_mut(&id) {
                                pointer.pos = pos;

                                if let PointerInteraction::Selecting {
                                    ref mut drag_start,
                                    dragged_handle,
                                    ref mut drag_accumulated_delta,
                                    ..
                                } = pointer.interaction
                                {
                                    let delta = pos - *drag_start;

                                    if !self.state.selected_object_indices.is_empty() {
                                        if let Some(handle) = dragged_handle {
                                            if let Some(&primary_idx) =
                                                self.state.selected_object_indices.first()
                                                && primary_idx < self.state.canvas.objects.len()
                                                && let Some(object) =
                                                    self.state.canvas.objects.get_mut(primary_idx)
                                            {
                                                object.transform(handle, delta, *drag_start, pos);
                                            }
                                        } else {
                                            // Move all selected objects by delta
                                            for &idx in &self.state.selected_object_indices.clone()
                                            {
                                                if idx < self.state.canvas.objects.len()
                                                    && let Some(object) =
                                                        self.state.canvas.objects.get_mut(idx)
                                                {
                                                    CanvasObject::move_object(object, delta);
                                                }
                                            }
                                            *drag_accumulated_delta += delta;
                                        }
                                    }

                                    *drag_start = pos;
                                }

                                if let PointerInteraction::MarqueeSelect {
                                    ref mut points, ..
                                } = pointer.interaction
                                {
                                    let min_dist = 2.0;
                                    if points
                                        .last()
                                        .is_none_or(|last| last.distance(pos) >= min_dist)
                                    {
                                        points.push(pos);
                                    }
                                }
                            }
                        }
                        CanvasTool::ObjectEraser | CanvasTool::PixelEraser => {
                            if let Some(pointer) = self.state.pointers.get_mut(&id) {
                                pointer.pos = pos;
                            }
                        }
                        CanvasTool::Insert => {
                            if let Some(pointer) = self.state.pointers.get_mut(&id) {
                                pointer.pos = pos;
                            }
                        }
                        _ => {}
                    },
                    TouchPhase::Ended | TouchPhase::Cancelled => match self.state.current_tool {
                        CanvasTool::View => {
                            self.state.pointers.remove(&id);
                            let panning_count = self
                                .state
                                .pointers
                                .values()
                                .filter(|p| {
                                    matches!(p.interaction, PointerInteraction::Panning { .. })
                                })
                                .count();
                            if panning_count < 2 {
                                self.state.pinch_state = None;
                            }
                        }
                        CanvasTool::Brush => {
                            brush_stroke_end(&mut self.state, id);
                        }
                        CanvasTool::Select => {
                            if let Some(pointer) = self.state.pointers.remove(&id) {
                                match pointer.interaction {
                                    PointerInteraction::Selecting {
                                        drag_accumulated_delta,
                                        drag_original_transforms,
                                        dragged_handle,
                                        ..
                                    } => {
                                        if dragged_handle.is_some() {
                                            // Resize: save TransformObject for single object
                                            if let Some(&(sel_idx, _)) =
                                                drag_original_transforms.first()
                                                && sel_idx < self.state.canvas.objects.len()
                                                && let Some(object) =
                                                    self.state.canvas.objects.get(sel_idx)
                                            {
                                                let new_transform = object.get_transform();
                                                let old_transform =
                                                    drag_original_transforms[0].1.clone();
                                                self.state.history.save_transform_object(
                                                    sel_idx,
                                                    old_transform,
                                                    new_transform,
                                                );
                                            }
                                        } else if drag_accumulated_delta != Vec2::ZERO {
                                            // Move: save MoveObject(s) for all selected objects
                                            let commands: Vec<HistoryCommand> =
                                                drag_original_transforms
                                                    .iter()
                                                    .map(|&(idx, _)| HistoryCommand::MoveObject {
                                                        index: idx,
                                                        old_position: -drag_accumulated_delta,
                                                        new_position: drag_accumulated_delta,
                                                    })
                                                    .collect();
                                            if commands.len() <= 1 {
                                                if let Some(cmd) = commands.into_iter().next() {
                                                    self.state.history.push_command(cmd);
                                                }
                                            } else {
                                                self.state.history.save_batch(commands);
                                            }
                                        }
                                    }
                                    PointerInteraction::MarqueeSelect {
                                        drag_start: _,
                                        mut points,
                                    } => {
                                        // Close the polygon (auto-connect start to end)
                                        if points.len() >= 2
                                            && points
                                                .first()
                                                .unwrap()
                                                .distance(*points.last().unwrap())
                                                > 0.5
                                        {
                                            points.push(*points.first().unwrap());
                                        }

                                        self.state.clear_selection();

                                        if points.len() >= 3 {
                                            // Simplify polygon for hit-testing performance
                                            let simplified = utils::simplify_polygon(&points, 4.0);

                                            // Collect matching objects based on mode
                                            let mode = self.state.marquee_match_mode;
                                            let intersecting: Vec<usize> = self
                                                .state
                                                .canvas
                                                .objects
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, obj)| {
                                                    if let CanvasObject::Stroke(s) = obj {
                                                        match mode {
                                                            MarqueeMatchMode::Overlapping => {
                                                                s.points.iter().any(|p| {
                                                                    utils::point_in_polygon(
                                                                        *p,
                                                                        &simplified,
                                                                    )
                                                                })
                                                            }
                                                            MarqueeMatchMode::Containing => {
                                                                s.points.iter().all(|p| {
                                                                    utils::point_in_polygon(
                                                                        *p,
                                                                        &simplified,
                                                                    )
                                                                })
                                                            }
                                                        }
                                                    } else {
                                                        match mode {
                                                            MarqueeMatchMode::Overlapping => {
                                                                utils::polygon_intersects_rect(
                                                                    &simplified,
                                                                    obj.bounding_box(),
                                                                )
                                                            }
                                                            MarqueeMatchMode::Containing => {
                                                                utils::polygon_contains_rect(
                                                                    &simplified,
                                                                    obj.bounding_box(),
                                                                )
                                                            }
                                                        }
                                                    }
                                                })
                                                .map(|(i, _)| i)
                                                .collect();
                                            for i in intersecting {
                                                self.state.selected_object_indices.push(i);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        CanvasTool::ObjectEraser | CanvasTool::PixelEraser => {
                            self.state.pointers.remove(&id);
                        }
                        CanvasTool::Insert => {
                            if let Some(pointer) = self.state.pointers.remove(&id)
                                && let PointerInteraction::ShapeInsert {
                                    start_pos,
                                    shape_type,
                                } = pointer.interaction
                            {
                                let end_pos = pointer.pos;
                                crate::utils::ui::create_shape_object(
                                    &mut self.state,
                                    shape_type,
                                    start_pos,
                                    end_pos,
                                );
                            }
                        }
                        _ => {}
                    },
                }

                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::CursorMoved {
                device_id: _,
                position,
            } => {
                self.state.cursor_position = position;
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}
