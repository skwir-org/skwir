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

use std::{cell::OnceCell, ptr::NonNull, sync::Arc};

use anyhow::{Context as _, Result};
use gpui::{DevicePixels, Mesh, MeshMaker, MeshPreparation, MeshStore, PaintMesh, Size};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBlendFactor, MTLBlendOperation, MTLBuffer, MTLClearColor, MTLCommandBuffer,
    MTLCommandEncoder, MTLCompareFunction, MTLCompileOptions, MTLCullMode,
    MTLDepthStencilDescriptor, MTLDepthStencilState, MTLDevice, MTLIndexType, MTLLibrary,
    MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLRenderCommandEncoder,
    MTLRenderPassDescriptor, MTLRenderPipelineDescriptor, MTLRenderPipelineState,
    MTLResourceOptions, MTLStorageMode, MTLStoreAction, MTLTexture, MTLTextureDescriptor,
    MTLTextureType, MTLTextureUsage, MTLVertexDescriptor, MTLVertexFormat, MTLVertexStepFunction,
    MTLViewport,
};

const SOURCE: &str = include_str!("shaders_mesh.metal");
/// Samples a pixel, as paths are drawn: every Metal GPU has four.
const SAMPLE_COUNT: usize = 4;
const COLOR_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;
const DEPTH_FORMAT: MTLPixelFormat = MTLPixelFormat::Depth32Float;
/// The largest side of a texture every Metal GPU allows.
const LARGEST_TARGET: u32 = 8192;
/// Where the vertex function finds the positions, normals and colors: after
/// its parameters, in buffer 0.
const POSITIONS: usize = 1;
const NORMALS: usize = 2;
const COLORS: usize = 3;

type Device = Retained<ProtocolObject<dyn MTLDevice>>;
type Texture = Retained<ProtocolObject<dyn MTLTexture>>;

/// A buffer the mesh store makes on its thread and hands to the renderer's.
pub(crate) struct Buffer(Retained<ProtocolObject<dyn MTLBuffer>>);

// SAFETY: Metal's buffers may be used from any thread. The store's thread
// fills each one before handing it over and never touches it again; only the
// renderer's thread binds it afterwards.
unsafe impl Send for Buffer {}

/// A mesh's buffers on the device.
pub(crate) struct Buffers {
    positions: Buffer,
    indices: Buffer,
    /// Its positions' colors, when it has some.
    colors: Option<Buffer>,
    count: usize,
}

/// How the device holds a mesh (`gpui::MeshMaker`).
#[derive(Clone)]
pub(crate) struct Maker {
    device: Device,
    options: MTLResourceOptions,
}

impl MeshMaker for Maker {
    type Buffers = Buffers;
    type Normals = Buffer;

    fn holds(&self, mesh: &Mesh) -> bool {
        let largest = self.device.maxBufferLength();
        [
            size_of_val(mesh.positions()),
            size_of_val(mesh.indices()),
            mesh.colors().map_or(0, size_of_val),
        ]
        .into_iter()
        .all(|bytes| bytes <= largest)
    }

    fn buffers(&self, mesh: &Mesh) -> Option<Buffers> {
        Some(Buffers {
            positions: self.buffer(mesh.positions())?,
            indices: self.buffer(mesh.indices())?,
            colors: match mesh.colors() {
                Some(colors) => Some(self.buffer(colors)?),
                None => None,
            },
            count: mesh.indices().len(),
        })
    }

    fn normals(&self, normals: &[[f32; 3]]) -> Option<Buffer> {
        self.buffer(normals)
    }
}

impl Maker {
    /// A buffer holding a copy of `data`; `None` when the device could not
    /// make it.
    fn buffer<T: Copy>(&self, data: &[T]) -> Option<Buffer> {
        // An empty buffer is refused: one byte stands for none.
        let length = size_of_val(data).max(1);
        let bytes = [0u8; 1];
        let pointer = if data.is_empty() {
            NonNull::from(&bytes).cast()
        } else {
            NonNull::from(data).cast()
        };
        // SAFETY: Metal copies `length` bytes from the live slice (or the
        // one byte standing for an empty one) before returning.
        unsafe {
            self.device
                .newBufferWithBytes_length_options(pointer, length, self.options)
        }
        .map(Buffer)
    }
}

struct Pipelines {
    draw: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    composite: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    depth: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
}

impl Pipelines {
    fn new(device: &ProtocolObject<dyn MTLDevice>) -> Result<Self> {
        let library = device
            .newLibraryWithSource_options_error(
                &NSString::from_str(SOURCE),
                Some(&MTLCompileOptions::new()),
            )
            .context("compiling the mesh shaders")?;
        let function = |name: &str| {
            library
                .newFunctionWithName(&NSString::from_str(name))
                .with_context(|| format!("locating function {name}"))
        };
        let (vertex, fragment) = (function("vs_mesh")?, function("fs_mesh")?);
        let (composite_vertex, composite_fragment) = (
            function("vs_mesh_composite")?,
            function("fs_mesh_composite")?,
        );

        let descriptor = MTLRenderPipelineDescriptor::new();
        descriptor.setLabel(Some(&NSString::from_str("meshes")));
        descriptor.setVertexFunction(Some(vertex.as_ref()));
        descriptor.setFragmentFunction(Some(fragment.as_ref()));
        // Positions, normals, colors: a buffer each.
        let vertices = MTLVertexDescriptor::new();
        for (location, (buffer, format, stride)) in [
            (POSITIONS, MTLVertexFormat::Float3, 12),
            (NORMALS, MTLVertexFormat::Float3, 12),
            (COLORS, MTLVertexFormat::UChar4Normalized, 4),
        ]
        .into_iter()
        .enumerate()
        {
            // SAFETY: The attribute and layout indices are below Metal's
            // 31, and the offset and stride describe the buffers bound.
            unsafe {
                let attribute = vertices.attributes().objectAtIndexedSubscript(location);
                attribute.setFormat(format);
                attribute.setOffset(0);
                attribute.setBufferIndex(buffer);
                let layout = vertices.layouts().objectAtIndexedSubscript(buffer);
                layout.setStride(stride);
                layout.setStepFunction(MTLVertexStepFunction::PerVertex);
                layout.setStepRate(1);
            }
        }
        descriptor.setVertexDescriptor(Some(&vertices));
        descriptor.setRasterSampleCount(SAMPLE_COUNT);
        descriptor.setDepthAttachmentPixelFormat(DEPTH_FORMAT);
        // SAFETY: Index zero is within the pipeline's color attachment array.
        let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
        color.setPixelFormat(COLOR_FORMAT);
        // What is drawn is written as it is: the target holds nothing else.
        color.setBlendingEnabled(false);
        let draw = device
            .newRenderPipelineStateWithDescriptor_error(&descriptor)
            .context("creating the mesh pipeline")?;

        let descriptor = MTLRenderPipelineDescriptor::new();
        descriptor.setLabel(Some(&NSString::from_str("mesh_composite")));
        descriptor.setVertexFunction(Some(composite_vertex.as_ref()));
        descriptor.setFragmentFunction(Some(composite_fragment.as_ref()));
        // SAFETY: Index zero is within the pipeline's color attachment array.
        let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
        color.setPixelFormat(COLOR_FORMAT);
        // What was drawn is premultiplied by its coverage: resolved edges
        // over nothing.
        color.setBlendingEnabled(true);
        color.setRgbBlendOperation(MTLBlendOperation::Add);
        color.setAlphaBlendOperation(MTLBlendOperation::Add);
        color.setSourceRGBBlendFactor(MTLBlendFactor::One);
        color.setSourceAlphaBlendFactor(MTLBlendFactor::One);
        color.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        color.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        let composite = device
            .newRenderPipelineStateWithDescriptor_error(&descriptor)
            .context("creating the mesh composite pipeline")?;

        let descriptor = MTLDepthStencilDescriptor::new();
        descriptor.setDepthCompareFunction(MTLCompareFunction::Less);
        descriptor.setDepthWriteEnabled(true);
        let depth = device
            .newDepthStencilStateWithDescriptor(&descriptor)
            .context("creating the mesh depth state")?;
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
    multisampled: Texture,
    depth: Texture,
    resolved: Texture,
}

impl Target {
    /// Kept in tile memory only, where the GPU can (Apple's): drawn and
    /// resolved in one pass. `None` when the device could not make it.
    fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        width: u32,
        height: u32,
        memoryless: bool,
    ) -> Option<Self> {
        let texture = |format, samples: usize, usage, memoryless| {
            let descriptor = MTLTextureDescriptor::new();
            // SAFETY: The dimensions are positive, within LARGEST_TARGET,
            // and the sample count one Metal GPUs all support.
            unsafe {
                descriptor.setWidth(width.max(1) as usize);
                descriptor.setHeight(height.max(1) as usize);
                if samples > 1 {
                    descriptor.setTextureType(MTLTextureType::Type2DMultisample);
                    descriptor.setSampleCount(samples);
                }
            }
            descriptor.setPixelFormat(format);
            descriptor.setStorageMode(if memoryless {
                MTLStorageMode::Memoryless
            } else {
                MTLStorageMode::Private
            });
            descriptor.setUsage(usage);
            device.newTextureWithDescriptor(&descriptor)
        };
        let target = MTLTextureUsage::RenderTarget;
        Some(Target {
            width,
            height,
            multisampled: texture(COLOR_FORMAT, SAMPLE_COUNT, target, memoryless)?,
            depth: texture(DEPTH_FORMAT, SAMPLE_COUNT, target, memoryless)?,
            resolved: texture(COLOR_FORMAT, 1, target | MTLTextureUsage::ShaderRead, false)?,
        })
    }
}

pub(crate) struct MeshPass {
    device: Device,
    memoryless: bool,
    /// Made when the first mesh is asked for; `None` if they cannot be.
    pipelines: OnceCell<Option<Pipelines>>,
    meshes: MeshStore<Maker>,
    target: Option<Target>,
}

impl MeshPass {
    /// `options` are the buffers', and `apple_gpu` whether the device can
    /// keep a texture in tile memory only.
    pub(crate) fn new(device: &Device, options: MTLResourceOptions, apple_gpu: bool) -> Self {
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
                    .inspect_err(|error| log::error!("meshes will not be drawn: {error:#}"))
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
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
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
            self.target = Target::new(&self.device, width, height, self.memoryless);
        }
        let (Some(Some(pipelines)), Some(target)) = (self.pipelines.get(), self.target.as_ref())
        else {
            return None;
        };
        // Flat until its normals are made.
        let smooth = paint.view.smooth && normals.is_some();
        let colored = buffers.colors.is_some();
        let params = framed.params(&paint.view, smooth, colored);

        let descriptor = MTLRenderPassDescriptor::new();
        // SAFETY: Index zero is within the render pass's color attachment array.
        let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
        color.setTexture(Some(&target.multisampled));
        color.setResolveTexture(Some(&target.resolved));
        color.setLoadAction(MTLLoadAction::Clear);
        color.setClearColor(MTLClearColor {
            red: 0.,
            green: 0.,
            blue: 0.,
            alpha: 0.,
        });
        // Only what is resolved is read.
        color.setStoreAction(MTLStoreAction::MultisampleResolve);
        let depth = descriptor.depthAttachment();
        depth.setTexture(Some(&target.depth));
        depth.setLoadAction(MTLLoadAction::Clear);
        depth.setClearDepth(1.);
        depth.setStoreAction(MTLStoreAction::DontCare);

        let encoder = command_buffer.renderCommandEncoderWithDescriptor(&descriptor)?;
        let [shown_width, shown_height] = framed.shown;
        encoder.setViewport(MTLViewport {
            originX: 0.,
            originY: 0.,
            width: f64::from(shown_width),
            height: f64::from(shown_height),
            znear: 0.,
            zfar: 1.,
        });
        encoder.setRenderPipelineState(&pipelines.draw);
        encoder.setDepthStencilState(Some(&pipelines.depth));
        // Either side may face the camera, as a mesh's winding is not
        // always kept.
        encoder.setCullMode(MTLCullMode::None);
        let bytes = size_of_val(&params);
        // Without normals or colors the shader reads none: the positions
        // stand in, as long as either would be.
        let normals = normals.unwrap_or(&buffers.positions);
        let colors = buffers.colors.as_ref().unwrap_or(&buffers.positions);
        // SAFETY: The parameters are copied from a live value into the slot
        // the shaders read them from; the buffers bound hold what the vertex
        // descriptor says, and the indices `count` u32s, each within the
        // positions (`gpui::Mesh` checks them).
        unsafe {
            encoder.setVertexBytes_length_atIndex(NonNull::from(&params).cast(), bytes, 0);
            encoder.setFragmentBytes_length_atIndex(NonNull::from(&params).cast(), bytes, 0);
            encoder.setVertexBuffer_offset_atIndex(Some(&buffers.positions.0), 0, POSITIONS);
            encoder.setVertexBuffer_offset_atIndex(Some(&normals.0), 0, NORMALS);
            encoder.setVertexBuffer_offset_atIndex(Some(&colors.0), 0, COLORS);
            encoder.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset(
                MTLPrimitiveType::Triangle,
                buffers.count,
                MTLIndexType::UInt32,
                &buffers.indices.0,
                0,
            );
        }
        encoder.endEncoding();
        Some(Drawn {
            composite: framed.composite(),
        })
    }

    /// Composite a mesh drawn into the target, pixel for pixel, with
    /// `encoder`.
    pub(crate) fn composite(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        drawn: &Drawn,
    ) {
        let (Some(Some(pipelines)), Some(target)) = (self.pipelines.get(), self.target.as_ref())
        else {
            return;
        };
        let bytes = size_of_val(&drawn.composite);
        let composite = NonNull::from(&drawn.composite).cast();
        encoder.setRenderPipelineState(&pipelines.composite);
        // SAFETY: The composite's parameters are copied from a live value
        // into the slot its shaders read; the texture is the target's,
        // resolved by the pass before this one.
        unsafe {
            encoder.setVertexBytes_length_atIndex(composite, bytes, 0);
            encoder.setFragmentBytes_length_atIndex(composite, bytes, 0);
            encoder.setFragmentTexture_atIndex(Some(&target.resolved), 0);
            encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::TriangleStrip, 0, 4);
        }
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
