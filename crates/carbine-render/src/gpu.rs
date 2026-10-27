use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use carbine_core::{DrawItem, MeshHandle, Scene};
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
fn versus(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return uniforms.view_proj * uniforms.model * vec4<f32>(position, 1.0);
}

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return uniforms.color;
}
"#;

/// Carbine black, the editor viewport background.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.086,
    g: 0.082,
    b: 0.078,
    a: 1.0,
};

const UNIFORM_BYTES: usize = std::mem::size_of::<Uniforms>();
const COPY_ROW_ALIGNMENT: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

#[derive(Debug, Clone)]
pub enum RenderError {
    NoAdapter(String),
    Device(String),
    Surface(String),
    Window(String),
    Readback(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(detail) => write!(
                formatter,
                "no Vulkan adapter was available ({detail}); install a Vulkan driver"
            ),
            Self::Device(detail) => {
                write!(formatter, "the GPU refused to create a device: {detail}")
            }
            Self::Surface(detail) => write!(formatter, "the surface could not be used: {detail}"),
            Self::Window(detail) => write!(formatter, "no window is available: {detail}"),
            Self::Readback(detail) => write!(formatter, "pixel readback failed: {detail}"),
        }
    }
}

impl std::error::Error for RenderError {}

pub fn create_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

/// A GPU device plus the caches that keep a frame cheap: one vertex and index
/// buffer per mesh, one pipeline per surface format.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
    meshes: HashMap<MeshHandle, GpuMesh>,
    pipelines: HashMap<wgpu::TextureFormat, Pipeline>,
}

struct GpuMesh {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    index_count: u32,
}

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// One uniform buffer and bind group per draw. Reusing a single buffer for a
    /// whole frame would make every object read the last upload instead of its own.
    slots: Vec<Slot>,
}

struct Slot {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Gpu {
    pub fn new(
        instance: wgpu::Instance,
        compatible_surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface,
            ..Default::default()
        }))
        .map_err(|error| RenderError::NoAdapter(error.to_string()))?;

        let name = {
            let info = adapter.get_info();
            format!("{} ({:?})", info.name, info.backend)
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("carbine"),
            ..Default::default()
        }))
        .map_err(|error| RenderError::Device(error.to_string()))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            adapter_name: name,
            meshes: HashMap::new(),
            pipelines: HashMap::new(),
        })
    }

    pub fn adapter(&self) -> &str {
        &self.adapter_name
    }

    /// A surface format that passes our unlit sRGB colours straight through.
    pub fn pick_format(
        &self,
        surface: &wgpu::Surface<'_>,
    ) -> Result<wgpu::TextureFormat, RenderError> {
        let capabilities = surface.get_capabilities(&self.adapter);
        let plain = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ];
        capabilities
            .formats
            .iter()
            .find(|format| plain.contains(format))
            .or_else(|| capabilities.formats.first())
            .copied()
            .ok_or_else(|| RenderError::Surface("the window supports no color format".into()))
    }

    pub fn configure_surface(
        &self,
        surface: &wgpu::Surface<'_>,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> wgpu::SurfaceConfiguration {
        let mut config = surface
            .get_default_config(&self.adapter, width.max(1), height.max(1))
            .expect("the surface lost its configuration options");
        config.format = format;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&self.device, &config);
        config
    }

    /// An offscreen RGBA8 target, drawn into and then copied back to the CPU.
    pub fn create_target(&self, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("carbine target"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// Records the scene and submits it, returning the submission index so the
    /// caller can present the texture it rendered into.
    pub fn draw(
        &mut self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        aspect: f32,
        scene: &Scene,
    ) -> Result<wgpu::SubmissionIndex, RenderError> {
        let items = scene.draw_items();
        let view_proj = scene.camera().view_projection(aspect);
        let pipeline = ensure_pipeline(&mut self.pipelines, &self.device, format, items.len());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("carbine frame"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("carbine frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline.pipeline);

            for (index, item) in items.iter().enumerate() {
                let mesh = ensure_mesh(&mut self.meshes, &self.device, item);
                let slot = &pipeline.slots[index];
                let uniforms = Uniforms {
                    view_proj: view_proj.to_cols_array_2d(),
                    model: item.model.to_cols_array_2d(),
                    color: [item.color[0], item.color[1], item.color[2], 1.0],
                };
                self.queue
                    .write_buffer(&slot.uniforms, 0, bytemuck::bytes_of(&uniforms));
                pass.set_bind_group(0, &slot.bind_group, &[]);
                pass.set_vertex_buffer(0, mesh.vertex.slice(..));
                pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        Ok(self.queue.submit([encoder.finish()]))
    }

    pub fn present(&self, texture: wgpu::SurfaceTexture) {
        self.queue.present(texture);
    }

    /// Copies a render target into CPU memory as packed RGBA8, row by row.
    pub fn read_pixels(
        &mut self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RenderError> {
        let bytes_per_row = (width * 4).next_multiple_of(COPY_ROW_ALIGNMENT);
        let size = u64::from(bytes_per_row) * u64::from(height);
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("carbine readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("carbine readback"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let index = self.queue.submit([encoder.finish()]);

        let view = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        view.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: None,
            })
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        receiver
            .recv()
            .map_err(|_| RenderError::Readback("the map callback was dropped".into()))?
            .map_err(|error| RenderError::Readback(error.to_string()))?;

        let padded = view
            .get_mapped_range()
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        let mut packed = vec![0u8; (width * height * 4) as usize];
        let run = (width * 4) as usize;
        for row in 0..height {
            let start = (row * bytes_per_row) as usize;
            let target = (row * width * 4) as usize..((row + 1) * width * 4) as usize;
            packed[target].copy_from_slice(&padded[start..start + run]);
        }
        drop(padded);
        staging.unmap();

        Ok(packed)
    }

    pub fn surface_texture_view(texture: &wgpu::SurfaceTexture) -> wgpu::TextureView {
        texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default())
    }
}

/// A render pass held open while objects are drawn into it.
fn ensure_pipeline<'a>(
    pipelines: &'a mut HashMap<wgpu::TextureFormat, Pipeline>,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    draw_count: usize,
) -> &'a mut Pipeline {
    let pipeline = pipelines.entry(format).or_insert_with(|| {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("carbine unlit"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("carbine unlit"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("versus"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERTEX_ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let layout = pipeline.get_bind_group_layout(0);
        Pipeline {
            pipeline,
            layout,
            slots: Vec::new(),
        }
    });

    while pipeline.slots.len() < draw_count {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("carbine uniforms"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("carbine uniforms"),
            layout: &pipeline.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        pipeline.slots.push(Slot {
            uniforms,
            bind_group,
        });
    }

    pipeline
}

fn ensure_mesh<'a>(
    meshes: &'a mut HashMap<MeshHandle, GpuMesh>,
    device: &wgpu::Device,
    item: &DrawItem<'_>,
) -> &'a GpuMesh {
    let handle = item.handle;
    meshes.entry(handle).or_insert_with(|| {
        let vertices: Vec<[f32; 3]> = item
            .mesh
            .positions
            .iter()
            .map(|position| position.to_array())
            .collect();
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("carbine vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("carbine indices"),
            contents: bytemuck::cast_slice(&item.mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        GpuMesh {
            vertex,
            index,
            index_count: item.mesh.indices.len() as u32,
        }
    })
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
