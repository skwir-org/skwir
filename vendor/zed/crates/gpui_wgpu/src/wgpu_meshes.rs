//! Meshes drawn in 3D (`gpui::PaintMesh`). Each is drawn in a pass of its
//! own into an offscreen target the size of what is shown of it,
//! multisampled as paths are and with a depth buffer, then composited into
//! the frame in draw order, as paths are.
//!
//! The meshes are kept by a `gpui::MeshStore`, which uploads them off the
//! window's thread and lets go of them in the first frame that does not
//! draw them; this is how wgpu holds and draws them. The target goes with
//! the last mesh drawn; the pipelines come with the first, never at start.

use std::sync::Arc;

use gpui::{DevicePixels, Mesh, MeshId, MeshMaker, MeshPreparation, MeshStore, PaintMesh, Size};
use wgpu::util::DeviceExt as _;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

struct Pipelines {
    draw: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    draw_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
}

/// A mesh's buffers on wgpu's device.
pub(crate) struct Buffers {
    positions: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Its positions' colors, when it has some.
    colors: Option<wgpu::Buffer>,
    count: u32,
}

/// How wgpu's device holds a mesh (`gpui::MeshMaker`).
#[derive(Clone)]
pub(crate) struct Maker(Arc<wgpu::Device>);

impl MeshMaker for Maker {
    type Buffers = Buffers;
    type Normals = wgpu::Buffer;

    fn holds(&self, mesh: &Mesh) -> bool {
        let largest = self.0.limits().max_buffer_size;
        [mesh.positions().len() * 12, mesh.indices().len() * 4]
            .into_iter()
            .all(|bytes| bytes as u64 <= largest)
    }

    fn buffers(&self, mesh: &Mesh) -> Option<Buffers> {
        let buffer = |label, contents: &[u8]| {
            self.0
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage: if label == "mesh_indices" {
                        wgpu::BufferUsages::INDEX
                    } else {
                        wgpu::BufferUsages::VERTEX
                    },
                })
        };
        // What wgpu cannot make goes to the device's error handler.
        Some(Buffers {
            positions: buffer("mesh_positions", bytemuck::cast_slice(mesh.positions())),
            indices: buffer("mesh_indices", bytemuck::cast_slice(mesh.indices())),
            colors: mesh
                .colors()
                .map(|colors| buffer("mesh_colors", bytemuck::cast_slice(colors))),
            count: mesh.indices().len() as u32,
        })
    }

    fn normals(&self, normals: &[[f32; 3]]) -> Option<wgpu::Buffer> {
        Some(
            self.0
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh_normals"),
                    contents: bytemuck::cast_slice(normals),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        )
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
    format: wgpu::TextureFormat,
    sample_count: u32,
    pipelines: Option<Pipelines>,
    meshes: MeshStore<Maker>,
    target: Option<Target>,
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
            format,
            sample_count,
            pipelines: None,
            meshes: MeshStore::new(Maker(device)),
            target: None,
        }
    }

    /// Make `mesh` ready to draw, with its normals when `smooth`
    /// (`MeshStore::prepare`).
    pub(crate) fn prepare_mesh(&mut self, mesh: &Arc<Mesh>, smooth: bool) -> MeshPreparation {
        self.meshes.prepare(mesh, smooth)
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
        let framed = paint.framed(frame, device.limits().max_texture_dimension_2d)?;
        let id = paint.mesh.id();
        let (buffers, normals) = self.meshes.drawn(id)?;
        // Flat until its normals are made.
        let smooth = paint.view.smooth && normals.is_some();
        let colored = buffers.colors.is_some();
        let [width, height] = framed.target;
        if self
            .target
            .as_ref()
            .is_none_or(|target| (target.width, target.height) != (width, height))
        {
            self.target = Some(self.make_target(device, width, height));
        }
        let format = self.format;
        let sample_count = self.sample_count;
        let pipelines = self
            .pipelines
            .get_or_insert_with(|| Self::make_pipelines(device, format, sample_count));
        let target = self.target.as_ref()?;

        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_params"),
            contents: bytemuck::cast_slice(&framed.params(&paint.view, smooth, colored)),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let composite = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_composite_params"),
            contents: bytemuck::cast_slice(&framed.composite()),
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
                        binding: 1,
                        resource: composite.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&target.resolved),
                    },
                ],
            }),
            shown: (framed.shown[0], framed.shown[1]),
        })
    }

    /// Draw a prepared mesh into the target.
    pub(crate) fn draw(&self, encoder: &mut wgpu::CommandEncoder, prepared: &Prepared) {
        let (Some(pipelines), Some(target), Some((buffers, normals))) = (
            self.pipelines.as_ref(),
            self.target.as_ref(),
            self.meshes.get(prepared.mesh),
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
        let normals = normals.unwrap_or(&buffers.positions);
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
        self.meshes.end_frame();
        if self.meshes.is_empty() {
            self.target = None;
        }
    }

    /// How many meshes have buffers, and whether there is a target.
    #[cfg(test)]
    pub(crate) fn held(&self) -> (usize, bool) {
        (self.meshes.len(), self.target.is_some())
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
                uniform(1, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
    /// The shader is valid, and reads the parameters where
    /// `gpui::MeshFrame` writes them.
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
        // Each member's first word, and the struct's words.
        let words = |name: &str| {
            let (handle, ty) = module
                .types
                .iter()
                .find(|(_, ty)| ty.name.as_deref() == Some(name))
                .expect("the shader declares it");
            let naga::TypeInner::Struct { members, .. } = &ty.inner else {
                panic!("{name} is a struct");
            };
            let at = members
                .iter()
                .map(|member| (member.name.clone().unwrap_or_default(), member.offset / 4))
                .collect::<Vec<_>>();
            (at, layouter[handle].size as usize / 4)
        };
        let named = |at: &[(&str, u32)]| {
            at.iter()
                .map(|&(name, word)| (name.to_owned(), word))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            words("MeshParams"),
            (
                named(&[
                    ("model_view", 0),
                    ("projection", 16),
                    ("ndc_scale", 32),
                    ("ndc_offset", 34),
                    ("color", 36),
                    ("light0", 40),
                    ("light1", 44),
                    ("ambient", 48),
                    ("perspective", 49),
                    ("shaded_smooth", 50),
                    ("colored", 51),
                ]),
                std::mem::size_of::<gpui::MeshParams>() / 4
            )
        );
        assert_eq!(
            words("MeshComposite"),
            (
                named(&[("origin", 0), ("size", 2), ("viewport", 4), ("pad", 6)]),
                std::mem::size_of::<gpui::MeshComposite>() / 4
            )
        );
    }

    /// The mesh shader as Direct3D 11 and Metal take it: naga's translations
    /// of `shaders_mesh.wgsl`, kept beside their renderers, so the three
    /// draw one shader. `GPUI_WRITE_MESH_SHADERS=1` writes them again.
    #[test]
    fn the_mesh_shader_s_translations_are_current() {
        use std::collections::BTreeMap;
        let module = naga::front::wgsl::parse_str(include_str!("shaders_mesh.wgsl"))
            .expect("shader should parse");
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("shader should validate");
        let binding = |binding| naga::ResourceBinding { group: 0, binding };

        // Shader model 5.0: the mesh's parameters in register b0, the
        // composite's in b1, the drawn mesh in t0.
        let register = |register| naga::back::hlsl::BindTarget {
            space: 0,
            register,
            binding_array_size: None,
            dynamic_storage_buffer_offsets_index: None,
            restrict_indexing: false,
        };
        let mut options = naga::back::hlsl::Options {
            shader_model: naga::back::hlsl::ShaderModel::V5_0,
            fake_missing_bindings: false,
            ..naga::back::hlsl::Options::default()
        };
        options.binding_map.insert(binding(0), register(0));
        options.binding_map.insert(binding(1), register(1));
        options.binding_map.insert(binding(2), register(0));
        let mut hlsl = String::new();
        naga::back::hlsl::Writer::new(&mut hlsl, &options, &Default::default())
            .write(&module, &info, None)
            .expect("HLSL");

        // Metal 2.0: the parameters in buffer 0 of each stage, the drawn
        // mesh in texture 0; the vertices from the pipeline's descriptor.
        let target = |buffer, texture| naga::back::msl::BindTarget {
            buffer,
            texture,
            ..Default::default()
        };
        let stage = |composite: bool| {
            let mut resources = BTreeMap::new();
            if composite {
                resources.insert(binding(1), target(Some(0), None));
                resources.insert(binding(2), target(None, Some(0)));
            } else {
                resources.insert(binding(0), target(Some(0), None));
            }
            naga::back::msl::EntryPointResources {
                resources,
                ..Default::default()
            }
        };
        let options = naga::back::msl::Options {
            lang_version: (2, 0),
            per_entry_point_map: [
                ("vs_mesh", false),
                ("fs_mesh", false),
                ("vs_mesh_composite", true),
                ("fs_mesh_composite", true),
            ]
            .into_iter()
            .map(|(name, composite)| (name.to_owned(), stage(composite)))
            .collect(),
            fake_missing_bindings: false,
            ..Default::default()
        };
        let pipeline = naga::back::msl::PipelineOptions {
            vertex_pulling_transform: false,
            ..Default::default()
        };
        let (msl, _) =
            naga::back::msl::write_string(&module, &info, &options, &pipeline).expect("MSL");

        let header = |comment: &str| {
            format!(
                "{comment} Made by naga from gpui_wgpu's shaders_mesh.wgsl: do not edit. Its\n{comment} test `the_mesh_shader_s_translations_are_current` makes it again.\n"
            )
        };
        let crates = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
        let kept = [
            (
                format!("{crates}/gpui_windows/src/shaders_mesh.hlsl"),
                header("//") + &hlsl,
            ),
            (
                format!("{crates}/gpui_apple/src/shaders_mesh.metal"),
                header("//") + &msl,
            ),
        ];
        for (path, made) in kept {
            if std::env::var_os("GPUI_WRITE_MESH_SHADERS").is_some() {
                std::fs::write(&path, &made).unwrap();
            }
            let current = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                current == made,
                "{path} is not naga's translation of the shader"
            );
        }
    }
}
