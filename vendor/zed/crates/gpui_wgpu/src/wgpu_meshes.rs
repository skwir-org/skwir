//! Meshes drawn in 3D (`gpui::PaintMesh`). Each is drawn in a pass of its
//! own into an offscreen target the size of what is shown of it,
//! multisampled as paths are and with a depth buffer, then composited into
//! the frame in draw order, as paths are.
//!
//! A mesh's buffers are made and filled on a thread of the pass's own, so a
//! large mesh never holds the window's thread: where the driver maps the
//! memory uploads go through uncached, 72 MB took a second. A frame draws a
//! mesh only once it is there (`prepare_mesh`, which the window asks before
//! painting it), and lets go of it in the first frame that does not draw
//! it. The target goes with the last mesh drawn; the pipelines come with the
//! first, never at start.

use std::sync::{Arc, mpsc};

use bytemuck::{Pod, Zeroable};
use collections::{FxHashMap, FxHashSet};
use gpui::{DevicePixels, Mesh, MeshId, MeshPreparation, MeshUploaded, PaintMesh, Size};
use wgpu::util::DeviceExt as _;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The target's sides grow and shrink in steps of this many pixels, so a
/// panel being resized does not make a target at every frame.
const TARGET_STEP: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshParams {
    model_view: [[f32; 4]; 4],
    projection: [[f32; 4]; 4],
    ndc_scale: [f32; 2],
    ndc_offset: [f32; 2],
    color: [f32; 4],
    light0: [f32; 4],
    light1: [f32; 4],
    ambient: f32,
    perspective: u32,
    smooth: u32,
    colored: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CompositeParams {
    origin: [f32; 2],
    size: [f32; 2],
    viewport: [f32; 2],
    pad: [f32; 2],
}

struct Pipelines {
    draw: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    draw_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
}

struct Buffers {
    positions: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Made once a frame shades the mesh smoothly.
    normals: Option<wgpu::Buffer>,
    /// Its positions' colors, when it has some.
    colors: Option<wgpu::Buffer>,
    count: u32,
    /// Whether the frame being drawn draws it.
    drawn: bool,
}

/// `mesh`'s normals for smooth shading, made and filled.
fn normals_of(device: &wgpu::Device, mesh: &Mesh) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mesh_normals"),
        contents: bytemuck::cast_slice(&mesh.smooth_normals()),
        usage: wgpu::BufferUsages::VERTEX,
    })
}

impl Buffers {
    /// `mesh`'s, made and filled, with its normals when `normals`.
    fn of(device: &wgpu::Device, mesh: &Mesh, normals: bool) -> Self {
        Buffers {
            positions: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh_positions"),
                contents: bytemuck::cast_slice(mesh.positions()),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh_indices"),
                contents: bytemuck::cast_slice(mesh.indices()),
                usage: wgpu::BufferUsages::INDEX,
            }),
            normals: normals.then(|| normals_of(device, mesh)),
            colors: mesh.colors().map(|colors| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh_colors"),
                    contents: bytemuck::cast_slice(colors),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            }),
            count: mesh.indices().len() as u32,
            drawn: false,
        }
    }
}

/// What an upload makes: a mesh's buffers, with its normals or not, or
/// the normals of a mesh already there.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Making {
    Mesh { normals: bool },
    Normals,
}

/// A mesh to upload, and what tells the window that asked once it is.
struct Upload {
    mesh: Arc<Mesh>,
    making: Making,
    uploaded: MeshUploaded,
}

/// What the uploader made, for a mesh (nothing for one no one held any
/// more).
enum Made {
    Mesh(MeshId, Option<Buffers>),
    Normals(MeshId, Option<wgpu::Buffer>),
}

/// The thread that uploads meshes, one after another: what it is given,
/// and what it made.
struct Uploader {
    uploads: mpsc::Sender<Upload>,
    made: mpsc::Receiver<Made>,
}

impl Uploader {
    fn start(device: Arc<wgpu::Device>) -> std::io::Result<Self> {
        let (uploads, asked) = mpsc::channel::<Upload>();
        let (made_by, made) = mpsc::channel();
        std::thread::Builder::new()
            .name("gpui-meshes".into())
            .spawn(move || {
                for Upload {
                    mesh,
                    making,
                    uploaded,
                } in asked
                {
                    // Held by nothing else: no frame will draw it.
                    let wanted = Arc::strong_count(&mesh) > 1;
                    let id = mesh.id();
                    let made = match making {
                        Making::Mesh { normals } => {
                            Made::Mesh(id, wanted.then(|| Buffers::of(&device, &mesh, normals)))
                        }
                        Making::Normals => {
                            Made::Normals(id, wanted.then(|| normals_of(&device, &mesh)))
                        }
                    };
                    // Handed over before the window hears of it, so the frame it
                    // draws finds it.
                    if made_by.send(made).is_err() {
                        break;
                    }
                    if wanted {
                        uploaded.done();
                    }
                }
            })?;
        Ok(Uploader { uploads, made })
    }
}

/// Where meshes are drawn: a multisampled colour and depth, resolved into a
/// texture the composite reads.
struct Target {
    width: u32,
    height: u32,
    _multisampled_texture: Option<wgpu::Texture>,
    multisampled: Option<wgpu::TextureView>,
    _depth_texture: wgpu::Texture,
    depth: wgpu::TextureView,
    _resolved_texture: wgpu::Texture,
    resolved: wgpu::TextureView,
}

/// A mesh of this frame, ready for its two passes.
pub(crate) struct Prepared {
    mesh: MeshId,
    draw: wgpu::BindGroup,
    composite: wgpu::BindGroup,
    /// The size of what is shown of it, in pixels: its viewport in the
    /// target.
    shown: (u32, u32),
}

pub(crate) struct MeshPass {
    device: Arc<wgpu::Device>,
    format: wgpu::TextureFormat,
    sample_count: u32,
    pipelines: Option<Pipelines>,
    buffers: FxHashMap<MeshId, Buffers>,
    target: Option<Target>,
    /// Started with the first upload.
    uploader: Option<Uploader>,
    /// The meshes being uploaded.
    uploading: FxHashSet<MeshId>,
    /// The meshes whose normals are being made.
    smoothing: FxHashSet<MeshId>,
    /// The last mesh too large for this device, said once.
    refused: Option<MeshId>,
}

impl MeshPass {
    /// Meshes drawn by `device` into frames of `format`, with
    /// `sample_count` samples a pixel.
    pub(crate) fn new(
        device: Arc<wgpu::Device>,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        Self {
            device,
            format,
            sample_count,
            pipelines: None,
            buffers: FxHashMap::default(),
            target: None,
            uploader: None,
            uploading: FxHashSet::default(),
            smoothing: FxHashSet::default(),
            refused: None,
        }
    }

    /// Make `mesh` ready to draw, uploading it unless it is there or on its
    /// way, its normals with it when `smooth`. Asked by the window before it
    /// paints the mesh. A mesh there already, its normals asked for, is drawn
    /// flat until they are made.
    pub(crate) fn prepare_mesh(&mut self, mesh: &Arc<Mesh>, smooth: bool) -> MeshPreparation {
        self.take_uploaded();
        let id = mesh.id();
        if let Some(buffers) = self.buffers.get(&id) {
            if !smooth || buffers.normals.is_some() {
                return MeshPreparation::Ready;
            }
            if self.smoothing.contains(&id) {
                return MeshPreparation::Flat(None);
            }
            let (preparation, uploaded) = MeshPreparation::flat_until();
            return match self.send(mesh, Making::Normals, uploaded) {
                Ok(()) => {
                    self.smoothing.insert(id);
                    preparation
                }
                Err(()) => MeshPreparation::Flat(None),
            };
        }
        if self.uploading.contains(&id) {
            return MeshPreparation::Pending;
        }
        let largest = self.device.limits().max_buffer_size;
        if [mesh.positions().len() * 12, mesh.indices().len() * 4]
            .into_iter()
            .any(|bytes| bytes as u64 > largest)
        {
            if self.refused != Some(id) {
                self.refused = Some(id);
                log::warn!(
                    "a mesh of {} triangles is larger than this GPU's buffers ({largest} bytes)",
                    mesh.triangles()
                );
            }
            return MeshPreparation::Never;
        }
        let (preparation, uploaded) = MeshPreparation::start();
        match self.send(mesh, Making::Mesh { normals: smooth }, uploaded) {
            Ok(()) => {
                self.uploading.insert(id);
                preparation
            }
            Err(()) => MeshPreparation::Never,
        }
    }

    /// Give the uploader `mesh` to make, starting it with the first.
    fn send(&mut self, mesh: &Arc<Mesh>, making: Making, uploaded: MeshUploaded) -> Result<(), ()> {
        if self.uploader.is_none() {
            match Uploader::start(Arc::clone(&self.device)) {
                Ok(uploader) => self.uploader = Some(uploader),
                Err(error) => {
                    log::error!("no thread to upload meshes: {error}");
                    return Err(());
                }
            }
        }
        let upload = Upload {
            mesh: Arc::clone(mesh),
            making,
            uploaded,
        };
        match self.uploader.as_ref().map(|u| u.uploads.send(upload)) {
            Some(Ok(())) => Ok(()),
            _ => Err(()),
        }
    }

    /// Take what the uploader made.
    fn take_uploaded(&mut self) {
        let Some(uploader) = &self.uploader else {
            return;
        };
        while let Ok(made) = uploader.made.try_recv() {
            match made {
                Made::Mesh(id, buffers) => {
                    self.uploading.remove(&id);
                    if let Some(mut buffers) = buffers {
                        // Kept through the frame being painted, which asked
                        // for it.
                        buffers.drawn = true;
                        self.buffers.insert(id, buffers);
                    }
                }
                Made::Normals(id, normals) => {
                    self.smoothing.remove(&id);
                    if let (Some(buffers), Some(normals)) = (self.buffers.get_mut(&id), normals) {
                        buffers.normals = Some(normals);
                    }
                }
            }
        }
    }

    /// The samples a pixel meshes are drawn with on `adapter` into frames
    /// of `format`: as many, up to four, as both it and depth allow.
    pub(crate) fn sample_count(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> u32 {
        let colour = adapter.get_texture_format_features(format);
        let depth = adapter.get_texture_format_features(DEPTH_FORMAT);
        [4, 2, 1]
            .into_iter()
            .find(|&n| {
                colour.flags.sample_count_supported(n) && depth.flags.sample_count_supported(n)
            })
            .unwrap_or(1)
    }

    /// Size the target for `paint` and write what its passes read. `None`
    /// when its mesh is not uploaded, or nothing of it shows in a frame of
    /// `frame`'s size.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        paint: &PaintMesh,
        frame: Size<DevicePixels>,
    ) -> Option<Prepared> {
        let bounds = paint.bounds;
        let shown = bounds.intersect(&paint.content_mask.bounds);
        let left = shown.origin.x.0.floor().max(0.);
        let top = shown.origin.y.0.floor().max(0.);
        let right = (shown.origin.x.0 + shown.size.width.0)
            .ceil()
            .min(frame.width.0 as f32);
        let bottom = (shown.origin.y.0 + shown.size.height.0)
            .ceil()
            .min(frame.height.0 as f32);
        let (width, height) = (bounds.size.width.0, bounds.size.height.0);
        if right <= left || bottom <= top || width <= 0. || height <= 0. {
            return None;
        }
        let shown_size = ((right - left) as u32, (bottom - top) as u32);
        let id = paint.mesh.id();
        let buffers = self.buffers.get_mut(&id)?;
        buffers.drawn = true;
        // Flat until its normals are made.
        let smooth = paint.view.smooth && buffers.normals.is_some();
        let colored = buffers.colors.is_some();
        let largest = device.limits().max_texture_dimension_2d;
        let step = |side: u32| {
            side.div_ceil(TARGET_STEP)
                .saturating_mul(TARGET_STEP)
                .min(largest)
        };
        let wanted = (step(shown_size.0), step(shown_size.1));
        if self
            .target
            .as_ref()
            .is_none_or(|target| (target.width, target.height) != wanted)
        {
            self.target = Some(self.make_target(device, wanted.0, wanted.1));
        }
        let format = self.format;
        let sample_count = self.sample_count;
        let pipelines = self
            .pipelines
            .get_or_insert_with(|| Self::make_pipelines(device, format, sample_count));
        let target = self.target.as_ref()?;

        // Clip space spans the bounds; the viewport spans what is shown.
        let (shown_width, shown_height) = (shown_size.0 as f32, shown_size.1 as f32);
        let view = &paint.view;
        let [light0, light1] = view.lights.map(|light| {
            [
                light.direction[0],
                light.direction[1],
                light.direction[2],
                light.strength,
            ]
        });
        let params = MeshParams {
            model_view: view.model_view,
            projection: view.projection,
            ndc_scale: [width / shown_width, height / shown_height],
            ndc_offset: [
                (2. * (bounds.origin.x.0 - left) + width) / shown_width - 1.,
                1. - (2. * (bounds.origin.y.0 - top) + height) / shown_height,
            ],
            color: [view.color[0], view.color[1], view.color[2], 1.],
            light0,
            light1,
            ambient: view.ambient,
            perspective: u32::from(view.perspective),
            smooth: u32::from(smooth),
            colored: u32::from(colored),
        };
        let composite = CompositeParams {
            origin: [left, top],
            size: [shown_width, shown_height],
            viewport: [frame.width.0 as f32, frame.height.0 as f32],
            pad: [0.; 2],
        };
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let composite = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_composite_params"),
            contents: bytemuck::bytes_of(&composite),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Some(Prepared {
            mesh: id,
            draw: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mesh_bind_group"),
                layout: &pipelines.draw_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                }],
            }),
            composite: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mesh_composite_bind_group"),
                layout: &pipelines.composite_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: composite.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&target.resolved),
                    },
                ],
            }),
            shown: shown_size,
        })
    }

    /// Draw a prepared mesh into the target.
    pub(crate) fn draw(&self, encoder: &mut wgpu::CommandEncoder, prepared: &Prepared) {
        let (Some(pipelines), Some(target), Some(buffers)) = (
            self.pipelines.as_ref(),
            self.target.as_ref(),
            self.buffers.get(&prepared.mesh),
        ) else {
            return;
        };
        let (view, resolve_target) = match &target.multisampled {
            Some(multisampled) => (multisampled, Some(&target.resolved)),
            None => (&target.resolved, None),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mesh_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    // Only what is resolved is read.
                    store: if resolve_target.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let (width, height) = prepared.shown;
        pass.set_viewport(0., 0., width as f32, height as f32, 0., 1.);
        pass.set_pipeline(&pipelines.draw);
        pass.set_bind_group(0, &prepared.draw, &[]);
        pass.set_vertex_buffer(0, buffers.positions.slice(..));
        // Without normals or colors the shader reads none: the positions
        // stand in, as long as either would be.
        let normals = buffers.normals.as_ref().unwrap_or(&buffers.positions);
        pass.set_vertex_buffer(1, normals.slice(..));
        let colors = buffers.colors.as_ref().unwrap_or(&buffers.positions);
        pass.set_vertex_buffer(2, colors.slice(..));
        pass.set_index_buffer(buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..buffers.count, 0, 0..1);
    }

    /// Composite a prepared mesh, drawn, into the frame `pass` draws.
    pub(crate) fn composite(&self, pass: &mut wgpu::RenderPass<'_>, prepared: &Prepared) {
        let Some(pipelines) = self.pipelines.as_ref() else {
            return;
        };
        pass.set_pipeline(&pipelines.composite);
        pass.set_bind_group(0, &prepared.composite, &[]);
        pass.draw(0..4, 0..1);
    }

    /// After a frame: let go of the meshes it did not draw, and of the
    /// target if it drew none.
    pub(crate) fn end_frame(&mut self) {
        self.buffers
            .retain(|_, buffers| std::mem::take(&mut buffers.drawn));
        if self.buffers.is_empty() {
            self.target = None;
        }
    }

    /// How many meshes have buffers, and whether there is a target.
    #[cfg(test)]
    pub(crate) fn held(&self) -> (usize, bool) {
        (self.buffers.len(), self.target.is_some())
    }

    fn make_target(&self, device: &wgpu::Device, width: u32, height: u32) -> Target {
        let texture = |label, format, sample_count, usage| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (texture, view)
        };
        let (multisampled_texture, multisampled) = if self.sample_count > 1 {
            let (texture, view) = texture(
                "mesh_multisampled",
                self.format,
                self.sample_count,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            (Some(texture), Some(view))
        } else {
            (None, None)
        };
        let (depth_texture, depth) = texture(
            "mesh_depth",
            DEPTH_FORMAT,
            self.sample_count,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let (resolved_texture, resolved) = texture(
            "mesh_resolved",
            self.format,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        Target {
            width,
            height,
            _multisampled_texture: multisampled_texture,
            multisampled,
            _depth_texture: depth_texture,
            depth,
            _resolved_texture: resolved_texture,
            resolved,
        }
    }

    fn make_pipelines(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Pipelines {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpui_mesh_shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders_mesh.wgsl").into()),
        });
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh_layout"),
            entries: &[uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh_composite_layout"),
            entries: &[
                uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let layout = |label, bind_group_layout: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(bind_group_layout)],
                immediate_size: 0,
            })
        };
        let primitive = |topology| wgpu::PrimitiveState {
            topology,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            // Either side may face the camera.
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        };
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("meshes"),
            layout: Some(&layout("meshes_layout", &draw_layout)),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_mesh"),
                // Positions, normals, colors: a buffer each.
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 12,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        }],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 12,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 1,
                        }],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 4,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Unorm8x4,
                            offset: 0,
                            shader_location: 2,
                        }],
                    }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_mesh"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: primitive(wgpu::PrimitiveTopology::TriangleList),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh_composite"),
            layout: Some(&layout("mesh_composite_layout", &composite_layout)),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_mesh_composite"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_mesh_composite"),
                // What was drawn is premultiplied by its coverage: resolved
                // edges over nothing.
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: primitive(wgpu::PrimitiveTopology::TriangleStrip),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Pipelines {
            draw,
            composite,
            draw_layout,
            composite_layout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader is valid, and reads the parameters as they are written.
    #[test]
    fn the_mesh_shader_reads_what_is_written() {
        let module = naga::front::wgsl::parse_str(include_str!("shaders_mesh.wgsl"))
            .expect("shader should parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("shader should validate");
        let mut layouter = naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).expect("types lay out");
        let size = |name: &str| {
            let (handle, _) = module
                .types
                .iter()
                .find(|(_, ty)| ty.name.as_deref() == Some(name))
                .expect("the shader declares it");
            layouter[handle].size as usize
        };
        assert_eq!(size("MeshParams"), std::mem::size_of::<MeshParams>());
        assert_eq!(
            size("MeshComposite"),
            std::mem::size_of::<CompositeParams>()
        );
    }
}
