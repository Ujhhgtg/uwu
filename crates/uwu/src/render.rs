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

/// WGSL shader that converts sRGB-encoded fragment output to linear light
/// for correct HDR presentation on Rgba16Float surfaces.
const HDR_CONVERSION_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) idx: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((idx << 1u) & 2u), f32(idx & 2u));
    return VertexOutput(
        vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0),
        uv,
    );
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { return c / 12.92; }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_linearize(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(input.uv.x, 1.0 - input.uv.y);
    let srgb = textureSample(input_tex, input_sampler, uv);
    let gain = 3.0;
    return vec4<f32>(
        srgb_to_linear(srgb.r) * gain,
        srgb_to_linear(srgb.g) * gain,
        srgb_to_linear(srgb.b) * gain,
        srgb.a,
    );
}
"#;

/// Per-frame state for the sRGB-to-linear conversion pass used on HDR surfaces.
pub struct HdrConversion {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

pub struct RenderState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'static>,
    pub hdr: bool,
    pub hdr_conversion: Option<HdrConversion>,
    pub scale_factor: f32,
    pub egui_renderer: EguiRenderer,
}

impl RenderState {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        window: &Window,
        width: u32,
        height: u32,
        optimization_policy: OptimizationPolicy,
        present_mode: wgpu::PresentMode,
        hdr_enabled: bool,
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

        // Detect HDR support
        let caps = surface.get_capabilities(&adapter);
        let hdr = hdr_enabled && caps.formats.contains(&TextureFormat::Rgba16Float);
        let (format, hdr_active) = if hdr {
            println!("hdr display detected, using Rgba16Float surface format");
            (TextureFormat::Rgba16Float, true)
        } else {
            if !caps.formats.contains(&TextureFormat::Bgra8UnormSrgb) {
                println!(
                    "warning: Bgra8UnormSrgb not in surface capabilities ({:?}), using first available",
                    caps.formats
                );
            }
            (TextureFormat::Bgra8UnormSrgb, false)
        };

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
            format,
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

        // Create the sRGB-to-linear conversion pipeline for HDR rendering
        let hdr_conversion = if hdr_active {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("hdr conversion shader"),
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(HDR_CONVERSION_SHADER)),
            });

            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("hdr conversion bind group layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });

            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("hdr conversion pipeline layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });

            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("hdr conversion sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            });

            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("hdr conversion pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_linearize"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                cache: None,
                multiview_mask: None,
            });

            Some(HdrConversion {
                pipeline,
                bind_group_layout,
                sampler,
            })
        } else {
            None
        };

        Self {
            device,
            queue,
            surface,
            surface_config,
            hdr: hdr_active,
            hdr_conversion,
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

    pub fn is_hdr(&self) -> bool {
        self.hdr
    }

    /// Applies sRGB-to-linear conversion to the surface texture after egui renders.
    /// Only needed for HDR (Rgba16Float) surfaces where no automatic sRGB conversion exists.
    pub fn apply_hdr_conversion(
        &self,
        encoder: &mut CommandEncoder,
        surface_texture: &wgpu::SurfaceTexture,
        width: u32,
        height: u32,
    ) {
        let Some(ref conv) = self.hdr_conversion else {
            return;
        };

        // Create a temporary texture to read the sRGB data from the surface
        let temp = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr conversion temp texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let temp_view = temp.create_view(&wgpu::TextureViewDescriptor::default());

        // Copy surface (RENDER_ATTACHMENT → COPY_SRC) to temp (COPY_DST)
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &surface_texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &temp,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        // Bind group for this frame's temp texture
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hdr conversion bind group"),
            layout: &conv.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&temp_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&conv.sampler),
                },
            ],
        });

        // Full-screen quad pass: read temp → sRGB→linear → write surface
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("hdr conversion pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rpass.set_pipeline(&conv.pipeline);
        rpass.set_bind_group(0, &bind_group, &[]);
        rpass.draw(0..3, 0..1);
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
