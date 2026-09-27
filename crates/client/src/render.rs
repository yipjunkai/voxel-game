use std::mem::size_of;
use std::sync::Arc;

use anyhow::anyhow;
use bytemuck::cast_slice;
use wgpu::util::DeviceExt;
use wgpu::{
    Backends, Buffer, Color, CommandEncoderDescriptor, CurrentSurfaceTexture, Device,
    DeviceDescriptor, Instance, InstanceDescriptor, LoadOp, Operations, PowerPreference, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RequestAdapterOptions,
    StoreOp, Surface, SurfaceColorSpace, SurfaceConfiguration, TextureFormat, TextureUsages,
    TextureViewDescriptor,
};
use winit::window::Window;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

const VERTICES: &[Vertex] = &[
    Vertex {
        position: [0.0, 0.5, 0.0],
        color: [1.0, 0.0, 0.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.0],
        color: [0.0, 1.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.0],
        color: [0.0, 0.0, 1.0],
    },
];

/// Everything that only exists once there is a window to draw into. Held behind
/// a single `Option` in `App`, so the whole set is present or absent together.
pub struct Renderer {
    window: Arc<Window>,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    render_pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    num_vertices: u32,
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    const fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let size = window.inner_size();

        // wgpu 30 dropped `Default` for this one; the display handle is only
        // wanted by the GL backend, which `PRIMARY` excludes.
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..InstanceDescriptor::new_without_display_handle()
        });

        // Owning the `Arc` rather than borrowing the window is what makes this
        // `Surface<'static>` — otherwise the lifetime would infect `Renderer`.
        let surface = instance.create_surface(window.clone())?;

        // Surface before adapter, which reads backwards: `compatible_surface`
        // is how wgpu rules out a GPU that cannot present to this window.
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;

        let info = adapter.get_info();
        log::info!(
            "adapter: {} — {:?} on {:?}",
            info.name,
            info.device_type,
            info.backend
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("device"),
            ..Default::default()
        }))?;

        let caps = surface.get_capabilities(&adapter);

        // sRGB formats gamma-correct on write, so colors land as authored
        // rather than washed out. An empty list means the adapter and surface
        // are not compatible at all, which the indexing below would panic on.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(TextureFormat::is_srgb)
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| anyhow!("surface and adapter share no texture format"))?;

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: SurfaceColorSpace::default(),
            width: size.width,
            height: size.height,
            present_mode: caps
                .present_modes
                .first()
                .copied()
                .ok_or_else(|| anyhow!("surface and adapter share no present mode"))?,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .ok_or_else(|| anyhow!("surface and adapter share no alpha mode"))?,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        log::info!("surface: {:?}, {:?}", config.format, config.present_mode);

        let render_pipeline = Self::create_render_pipeline(&device, config.format);

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        #[allow(clippy::cast_possible_truncation)]
        let num_vertices = VERTICES.len() as u32;

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            render_pipeline,
            vertex_buffer,
            num_vertices,
        })
    }

    fn create_render_pipeline(device: &Device, format: TextureFormat) -> RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),     // 1.
                buffers: &[Some(Vertex::desc())], // 2.
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                // 3.
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    // 4.
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList, // 1.
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw, // 2.
                cull_mode: Some(wgpu::Face::Back),
                // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
                polygon_mode: wgpu::PolygonMode::Fill,
                // Requires Features::DEPTH_CLIP_CONTROL
                unclipped_depth: false,
                // Requires Features::CONSERVATIVE_RASTERIZATION
                conservative: false,
            },
            depth_stencil: None, // 1.
            multisample: wgpu::MultisampleState {
                count: 1,                         // 2.
                mask: !0,                         // 3.
                alpha_to_coverage_enabled: false, // 4.
            },
            multiview_mask: None, // 5.
            cache: None,          // 6.
        })
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Rebuild the swapchain at a new size. Its images are fixed-size textures,
    /// so they have to be recreated whenever the window stops matching them.
    ///
    /// A zero dimension — a minimized window on Windows — fails validation, so
    /// hold the last good config until the window comes back.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// The surface went stale — rebuild it against the window's current size,
    /// which may have changed without a `Resized` event reaching us yet.
    fn reconfigure(&mut self) {
        let size = self.window.inner_size();
        self.resize(size.width, size.height);
    }

    /// Acquire a swapchain image, clear it, and hand it to the compositor.
    pub fn render(&mut self) {
        // Queued before the acquire below, not after: every path out of this
        // function still needs a next frame to recover on.
        self.window.request_redraw();

        let (frame, suboptimal) = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => (frame, false),
            // Usable, but the swapchain has drifted from the surface.
            CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            // No image to draw into, and reconfiguring is the documented fix.
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                self.reconfigure();
                return;
            }
            // Transient. Skip the frame.
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return,
            CurrentSurfaceTexture::Validation => {
                log::error!("surface validation error while acquiring a frame");
                return;
            }
        };

        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("frame"),
            });

        // The pass borrows `encoder` mutably and writes itself into it when it
        // drops, so it has to be dead before `finish()` can take `encoder` by
        // value. Hence the block.
        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        // Linear values: an sRGB surface encodes them on write,
                        // so this displays lighter than the numbers suggest.
                        load: LoadOp::Clear(Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            render_pass.set_pipeline(&self.render_pipeline); // 2.
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.draw(0..self.num_vertices, 0..1); // 3.
        }

        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);

        // Deferred until after present so the swapchain is not rebuilt while
        // one of its images is still checked out.
        if suboptimal {
            self.reconfigure();
        }
    }
}
