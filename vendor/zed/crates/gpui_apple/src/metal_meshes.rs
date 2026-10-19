//! Meshes drawn in 3D (`gpui::PaintMesh`) by Metal, as gpui_wgpu draws
//! them: each in a pass of its own into an offscreen target the size of what
//! is shown of it (`gpui::MeshFrame`), multisampled as paths are and with a
//! depth buffer, then composited into the frame in draw order.
//!
//! The shaders are naga's translation of gpui_wgpu's `shaders_mesh.wgsl`
//! (`shaders_mesh.metal`, kept current by a test there), compiled when the
//! first mesh is asked for. The meshes are kept by a `gpui::MeshStore`,
//! which makes their buffers on a thread of its own and lets go of them in
//! the first frame that does not draw them; the target goes with the last
//! mesh drawn.

use std::{cell::OnceCell, ffi::c_void, sync::Arc};

use gpui::{DevicePixels, Mesh, MeshMaker, MeshPreparation, MeshStore, PaintMesh, Size};
use metal::{MTLPixelFormat, MTLResourceOptions, NSUInteger};

const SOURCE: &str = include_str!("shaders_mesh.metal");
/// Samples a pixel, as paths are drawn: every Metal GPU has four.
const SAMPLE_COUNT: NSUInteger = 4;
const COLOR_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;
const DEPTH_FORMAT: MTLPixelFormat = MTLPixelFormat::Depth32Float;
/// The largest side of a texture every Metal GPU allows.
const LARGEST_TARGET: u32 = 8192;
/// Where the vertex function finds the positions, normals and colors: after
/// its parameters, in buffer 0.
const POSITIONS: NSUInteger = 1;
const NORMALS: NSUInteger = 2;
const COLORS: NSUInteger = 3;

/// A mesh's buffers on the device.
pub(crate) struct Buffers {
    positions: metal::Buffer,
    indices: metal::Buffer,
    /// Its positions' colors, when it has some.
    colors: Option<metal::Buffer>,
    count: NSUInteger,
}

/// How the device holds a mesh (`gpui::MeshMaker`).
#[derive(Clone)]
pub(crate) struct Maker {
    device: metal::Device,
    options: MTLResourceOptions,
}

impl MeshMaker for Maker {
    type Buffers = Buffers;
    type Normals = metal::Buffer;

    fn holds(&self, mesh: &Mesh) -> bool {
        let largest = self.device.max_buffer_length();
        [
            size_of_val(mesh.positions()),
            size_of_val(mesh.indices()),
            mesh.colors().map_or(0, size_of_val),
        ]
        .into_iter()
        .all(|bytes| bytes as NSUInteger <= largest)
    }

    fn buffers(&self, mesh: &Mesh) -> Option<Buffers> {
        Some(Buffers {
            positions: self.buffer(mesh.positions()),
            indices: self.buffer(mesh.indices()),
            colors: mesh.colors().map(|colors| self.buffer(colors)),
            count: mesh.indices().len() as NSUInteger,
        })
    }

    fn normals(&self, normals: &[[f32; 3]]) -> Option<metal::Buffer> {
        Some(self.buffer(normals))
    }
}

impl Maker {
    /// A buffer holding a copy of `data`.
    fn buffer<T: Copy>(&self, data: &[T]) -> metal::Buffer {
        self.device.new_buffer_with_data(
            data.as_ptr().cast::<c_void>(),
            size_of_val(data) as NSUInteger,
            self.options,
        )
    }
}

struct Pipelines {
    draw: metal::RenderPipelineState,
    composite: metal::RenderPipelineState,
    depth: metal::DepthStencilState,
}

impl Pipelines {
    fn new(device: &metal::DeviceRef) -> Result<Self, String> {
        let library = device.new_library_with_source(SOURCE, &metal::CompileOptions::new())?;
        let function = |name| library.get_function(name, None);
        let (vertex, fragment) = (function("vs_mesh")?, function("fs_mesh")?);
        let (composite_vertex, composite_fragment) = (
            function("vs_mesh_composite")?,
            function("fs_mesh_composite")?,
        );

        let descriptor = metal::RenderPipelineDescriptor::new();
        descriptor.set_label("meshes");
        descriptor.set_vertex_function(Some(vertex.as_ref()));
        descriptor.set_fragment_function(Some(fragment.as_ref()));
        // Positions, normals, colors: a buffer each.
        let vertices = metal::VertexDescriptor::new();
        for (location, (buffer, format, stride)) in [
            (POSITIONS, metal::MTLVertexFormat::Float3, 12),
            (NORMALS, metal::MTLVertexFormat::Float3, 12),
            (COLORS, metal::MTLVertexFormat::UChar4Normalized, 4),
        ]
        .into_iter()
        .enumerate()
        {
            let attribute = vertices
                .attributes()
                .object_at(location as NSUInteger)
                .ok_or("no vertex attribute")?;
            attribute.set_format(format);
            attribute.set_offset(0);
            attribute.set_buffer_index(buffer);
            let layout = vertices
                .layouts()
                .object_at(buffer)
                .ok_or("no vertex layout")?;
            layout.set_stride(stride);
            layout.set_step_function(metal::MTLVertexStepFunction::PerVertex);
            layout.set_step_rate(1);
        }
        descriptor.set_vertex_descriptor(Some(vertices));
        descriptor.set_raster_sample_count(SAMPLE_COUNT);
        descriptor.set_depth_attachment_pixel_format(DEPTH_FORMAT);
        let color = descriptor
            .color_attachments()
            .object_at(0)
            .ok_or("no color attachment")?;
        color.set_pixel_format(COLOR_FORMAT);
        // What is drawn is written as it is: the target holds nothing else.
        color.set_blending_enabled(false);
        let draw = device.new_render_pipeline_state(&descriptor)?;

        let descriptor = metal::RenderPipelineDescriptor::new();
        descriptor.set_label("mesh_composite");
        descriptor.set_vertex_function(Some(composite_vertex.as_ref()));
        descriptor.set_fragment_function(Some(composite_fragment.as_ref()));
        let color = descriptor
            .color_attachments()
            .object_at(0)
            .ok_or("no color attachment")?;
        color.set_pixel_format(COLOR_FORMAT);
        // What was drawn is premultiplied by its coverage: resolved edges
        // over nothing.
        color.set_blending_enabled(true);
        color.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
        color.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
        color.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
        color.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
        color.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
        color.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
        let composite = device.new_render_pipeline_state(&descriptor)?;

        let descriptor = metal::DepthStencilDescriptor::new();
        descriptor.set_depth_compare_function(metal::MTLCompareFunction::Less);
        descriptor.set_depth_write_enabled(true);
        let depth = device.new_depth_stencil_state(&descriptor);
        Ok(Pipelines {
            draw,
            composite,
            depth,
        })
    }
}

/// Where meshes are drawn: a multisampled colour and depth, resolved into a
/// texture the composite reads.
struct Target {
    width: u32,
    height: u32,
    multisampled: metal::Texture,
    depth: metal::Texture,
    resolved: metal::Texture,
}

impl Target {
    /// Kept in tile memory only, where the GPU can (Apple's): drawn and
    /// resolved in one pass.
    fn new(device: &metal::DeviceRef, width: u32, height: u32, memoryless: bool) -> Self {
        let texture = |format, samples, usage, memoryless| {
            let descriptor = metal::TextureDescriptor::new();
            descriptor.set_width(width.max(1) as NSUInteger);
            descriptor.set_height(height.max(1) as NSUInteger);
            descriptor.set_pixel_format(format);
            if samples > 1 {
                descriptor.set_texture_type(metal::MTLTextureType::D2Multisample);
                descriptor.set_sample_count(samples);
            }
            descriptor.set_storage_mode(if memoryless {
                metal::MTLStorageMode::Memoryless
            } else {
                metal::MTLStorageMode::Private
            });
            descriptor.set_usage(usage);
            device.new_texture(&descriptor)
        };
        let target = metal::MTLTextureUsage::RenderTarget;
        Target {
            width,
            height,
            multisampled: texture(COLOR_FORMAT, SAMPLE_COUNT, target, memoryless),
            depth: texture(DEPTH_FORMAT, SAMPLE_COUNT, target, memoryless),
            resolved: texture(
                COLOR_FORMAT,
                1,
                target | metal::MTLTextureUsage::ShaderRead,
                false,
            ),
        }
    }
}

pub(crate) struct MeshPass {
    device: metal::Device,
    memoryless: bool,
    /// Made when the first mesh is asked for; `None` if they cannot be.
    pipelines: OnceCell<Option<Pipelines>>,
    meshes: MeshStore<Maker>,
    target: Option<Target>,
}

impl MeshPass {
    /// `options` are the buffers', and `apple_gpu` whether the device can
    /// keep a texture in tile memory only.
    pub(crate) fn new(
        device: &metal::Device,
        options: MTLResourceOptions,
        apple_gpu: bool,
    ) -> Self {
        Self {
            device: device.clone(),
            memoryless: apple_gpu,
            pipelines: OnceCell::new(),
            meshes: MeshStore::new(Maker {
                device: device.clone(),
                options,
            }),
            target: None,
        }
    }

    fn pipelines(&self) -> Option<&Pipelines> {
        self.pipelines
            .get_or_init(|| {
                Pipelines::new(&self.device)
                    .inspect_err(|error| log::error!("meshes will not be drawn: {error}"))
                    .ok()
            })
            .as_ref()
    }

    /// Make `mesh` ready to draw, with its normals when `smooth`
    /// (`MeshStore::prepare`); never, when the shaders cannot be made.
    pub(crate) fn prepare_mesh(&mut self, mesh: &Arc<Mesh>, smooth: bool) -> MeshPreparation {
        if self.pipelines().is_none() {
            return MeshPreparation::Never;
        }
        self.meshes.prepare(mesh, smooth)
    }

    /// Draw `paint` into the target, for a frame of `frame`'s size, in a
    /// pass of its own in `command_buffer`, which encodes nothing else
    /// meanwhile; `None` when nothing of it is drawn.
    pub(crate) fn draw(
        &mut self,
        command_buffer: &metal::CommandBufferRef,
        paint: &PaintMesh,
        frame: Size<DevicePixels>,
    ) -> Option<Drawn> {
        let framed = paint.framed(frame, LARGEST_TARGET)?;
        let (buffers, normals) = self.meshes.drawn(paint.mesh.id())?;
        let [width, height] = framed.target;
        if self
            .target
            .as_ref()
            .is_none_or(|target| (target.width, target.height) != (width, height))
        {
            self.target = None;
            self.target = Some(Target::new(&self.device, width, height, self.memoryless));
        }
        let (Some(Some(pipelines)), Some(target)) = (self.pipelines.get(), self.target.as_ref())
        else {
            return None;
        };
        // Flat until its normals are made.
        let smooth = paint.view.smooth && normals.is_some();
        let colored = buffers.colors.is_some();
        let params = framed.params(&paint.view, smooth, colored);

        let descriptor = metal::RenderPassDescriptor::new();
        let color = descriptor.color_attachments().object_at(0)?;
        color.set_texture(Some(&target.multisampled));
        color.set_resolve_texture(Some(&target.resolved));
        color.set_load_action(metal::MTLLoadAction::Clear);
        color.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));
        // Only what is resolved is read.
        color.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        let depth = descriptor.depth_attachment()?;
        depth.set_texture(Some(&target.depth));
        depth.set_load_action(metal::MTLLoadAction::Clear);
        depth.set_clear_depth(1.);
        depth.set_store_action(metal::MTLStoreAction::DontCare);

        let encoder = command_buffer.new_render_command_encoder(descriptor);
        let [shown_width, shown_height] = framed.shown;
        encoder.set_viewport(metal::MTLViewport {
            originX: 0.,
            originY: 0.,
            width: f64::from(shown_width),
            height: f64::from(shown_height),
            znear: 0.,
            zfar: 1.,
        });
        encoder.set_render_pipeline_state(&pipelines.draw);
        encoder.set_depth_stencil_state(&pipelines.depth);
        // Either side may face the camera, as a mesh's winding is not
        // always kept.
        encoder.set_cull_mode(metal::MTLCullMode::None);
        let bytes = size_of_val(&params) as NSUInteger;
        encoder.set_vertex_bytes(0, bytes, params.as_ptr().cast());
        encoder.set_fragment_bytes(0, bytes, params.as_ptr().cast());
        // Without normals or colors the shader reads none: the positions
        // stand in, as long as either would be.
        encoder.set_vertex_buffer(POSITIONS, Some(&buffers.positions), 0);
        encoder.set_vertex_buffer(NORMALS, Some(normals.unwrap_or(&buffers.positions)), 0);
        let colors = buffers.colors.as_ref().unwrap_or(&buffers.positions);
        encoder.set_vertex_buffer(COLORS, Some(colors), 0);
        encoder.draw_indexed_primitives(
            metal::MTLPrimitiveType::Triangle,
            buffers.count,
            metal::MTLIndexType::UInt32,
            &buffers.indices,
            0,
        );
        encoder.end_encoding();
        Some(Drawn {
            composite: framed.composite(),
        })
    }

    /// Composite a mesh drawn into the target, pixel for pixel, with
    /// `encoder`.
    pub(crate) fn composite(&self, encoder: &metal::RenderCommandEncoderRef, drawn: &Drawn) {
        let (Some(Some(pipelines)), Some(target)) = (self.pipelines.get(), self.target.as_ref())
        else {
            return;
        };
        let bytes = size_of_val(&drawn.composite) as NSUInteger;
        let composite = drawn.composite.as_ptr().cast();
        encoder.set_render_pipeline_state(&pipelines.composite);
        encoder.set_vertex_bytes(0, bytes, composite);
        encoder.set_fragment_bytes(0, bytes, composite);
        encoder.set_fragment_texture(0, Some(&target.resolved));
        encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
    }

    /// After a frame: let go of the meshes it did not draw, and of the
    /// target if it drew none.
    pub(crate) fn end_frame(&mut self) {
        self.meshes.end_frame();
        if self.meshes.is_empty() {
            self.target = None;
        }
    }
}

/// A mesh drawn into the target, to be composited.
pub(crate) struct Drawn {
    composite: gpui::MeshComposite,
}
