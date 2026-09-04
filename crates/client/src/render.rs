use std::sync::Arc;

use anyhow::anyhow;
use wgpu::{
    Backends, Color, CommandEncoderDescriptor, CurrentSurfaceTexture, Device, DeviceDescriptor,
    Instance, InstanceDescriptor, LoadOp, Operations, PowerPreference, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RequestAdapterOptions, StoreOp, Surface,
    SurfaceColorSpace, SurfaceConfiguration, TextureFormat, TextureUsages, TextureViewDescriptor,
};
use winit::window::Window;

/// Everything that only exists once there is a window to draw into. Held behind
/// a single `Option` in `App`, so the whole set is present or absent together.
pub struct Renderer {
    window: Arc<Window>,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
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
            present_mode: caps.present_modes[0],
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        log::info!("surface: {:?}, {:?}", config.format, config.present_mode);

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
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
            let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("clear"),
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
