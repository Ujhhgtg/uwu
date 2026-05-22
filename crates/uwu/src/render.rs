use egui::Context;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::ExperimentalFeatures;
use egui_wgpu::wgpu::{CommandEncoder, Device, Queue, StoreOp, TextureFormat, TextureView};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::State;
use wgpu::TextureUsages;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::state::OptimizationPolicy;
use crate::utils;

pub struct RenderState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'static>,
    pub scale_factor: f32,
    pub egui_renderer: EguiRenderer,
}

impl RenderState {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        window: &Window,
        width: u32,
        height: u32,
        optimization_policy: OptimizationPolicy,
        present_mode: wgpu::PresentMode,
    ) -> Self {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("failed to find an appropriate adapter");

        let info = adapter.get_info();
        println!("using gpu device: {}", info.name);
        println!("using render backend: {}", info.backend);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::default(),
                required_limits: wgpu::Limits::default(),
                memory_hints: match optimization_policy {
                    OptimizationPolicy::Performance => wgpu::MemoryHints::Performance,
                    OptimizationPolicy::ResourceUsage => wgpu::MemoryHints::MemoryUsage,
                },
                trace: wgpu::Trace::Off,
                experimental_features: ExperimentalFeatures::default(),
            })
            .await
            .expect("failed to create device");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            format: TextureFormat::Bgra8UnormSrgb,
            width,
            height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
        };

        surface.configure(&device, &surface_config);

        const SCALE_FACTOR: f32 = 1.0; // TODO: modifying this to non-1.0 values breaks most stuff

        let egui_renderer = EguiRenderer::new(
            &device,
            surface_config.format,
            None,
            1,
            window,
            SCALE_FACTOR,
        );

        Self {
            device,
            queue,
            surface,
            surface_config,
            egui_renderer,
            scale_factor: SCALE_FACTOR,
        }
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub fn set_present_mode(&mut self, present_mode: wgpu::PresentMode) {
        self.surface_config.present_mode = present_mode;
        self.surface.configure(&self.device, &self.surface_config);
    }
}

pub struct EguiRenderer {
    state: State,
    renderer: Renderer,
    frame_started: bool,
    pixels_per_point: f32,
}

impl EguiRenderer {
    pub fn context(&self) -> &Context {
        self.state.egui_ctx()
    }

    pub fn new(
        device: &Device,
        output_color_format: TextureFormat,
        output_depth_format: Option<TextureFormat>,
        msaa_samples: u32,
        window: &Window,
        pixels_per_point: f32,
    ) -> EguiRenderer {
        let mut egui_context = Context::default();

        utils::ui::setup_fonts(&mut egui_context);

        let egui_state = egui_winit::State::new(
            egui_context.clone(),
            egui::viewport::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(2 * 1024), // default dimension is 2048
        );
        let egui_renderer = Renderer::new(
            device,
            output_color_format,
            RendererOptions {
                depth_stencil_format: output_depth_format,
                msaa_samples,
                dithering: true,
                predictable_texture_filtering: false,
            },
        );
        egui_context.set_pixels_per_point(pixels_per_point);
        egui_context.memory_mut(|memory| {
            memory.options.tessellation_options.prerasterized_discs = true;
            memory.options.tessellation_options.parallel_tessellation = true;
            memory.options.zoom_with_keyboard = false;
        });

        EguiRenderer {
            state: egui_state,
            renderer: egui_renderer,
            frame_started: false,
            pixels_per_point,
        }
    }

    pub fn handle_input(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.state.on_window_event(window, event).repaint
    }

    #[cfg_attr(feature = "profiling", profiling::function)]
    pub fn begin_frame(&mut self, window: &Window) {
        let raw_input = self.state.take_egui_input(window);
        self.state.egui_ctx().begin_pass(raw_input);
        self.frame_started = true;
    }

    #[cfg_attr(feature = "profiling", profiling::function)]
    pub fn end_frame_and_draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        window: &Window,
        window_surface_view: &TextureView,
        screen_descriptor: ScreenDescriptor,
    ) {
        if !self.frame_started {
            panic!("begin_frame must be called before end_frame_and_draw is called");
        }

        let full_output = self.state.egui_ctx().end_pass();

        self.state
            .handle_platform_output(window, full_output.platform_output);

        let tris = {
            #[cfg(feature = "profiling")]
            profiling::scope!("egui::tessellate");
            self.state
                .egui_ctx()
                .tessellate(full_output.shapes, self.pixels_per_point)
        };
        {
            #[cfg(feature = "profiling")]
            profiling::scope!("egui::update_textures");
            for (id, image_delta) in &full_output.textures_delta.set {
                self.renderer
                    .update_texture(device, queue, *id, image_delta);
            }
        }
        {
            #[cfg(feature = "profiling")]
            profiling::scope!("egui::update_buffers");
            self.renderer
                .update_buffers(device, queue, encoder, &tris, &screen_descriptor);
        }

        let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui main render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: window_surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0_f64,
                        g: 0.0_f64,
                        b: 0.0_f64,
                        a: 0.0_f64,
                    }),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        {
            #[cfg(feature = "profiling")]
            profiling::scope!("egui::render");
            self.renderer
                .render(&mut rpass.forget_lifetime(), &tris, &screen_descriptor);
        }
        {
            #[cfg(feature = "profiling")]
            profiling::scope!("egui::free_textures");
            for x in &full_output.textures_delta.free {
                self.renderer.free_texture(x)
            }
        }

        self.frame_started = false;
    }
}
