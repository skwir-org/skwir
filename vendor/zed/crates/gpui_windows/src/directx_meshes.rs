//! Meshes drawn in 3D (`gpui::PaintMesh`) by Direct3D 11, as gpui_wgpu draws
//! them: each in a pass of its own into an offscreen target the size of what
//! is shown of it (`gpui::MeshFrame`), multisampled as paths are and with a
//! depth buffer, then composited into the frame in draw order.
//!
//! The shaders are naga's translation of gpui_wgpu's `shaders_mesh.wgsl`
//! (`shaders_mesh.hlsl`, kept current by a test there), compiled once a
//! process when the first mesh is asked for. The meshes are kept by a
//! `gpui::MeshStore`, which makes their buffers on a thread of its own (the
//! device is free-threaded) and lets go of them in the first frame that does
//! not draw them; the target goes with the last mesh drawn.

use std::{cell::OnceCell, ffi::c_void, slice, sync::Arc, sync::OnceLock};

use anyhow::{Context as _, Result};
use gpui::{
    DevicePixels, Mesh, MeshComposite, MeshMaker, MeshParams, MeshPreparation, MeshStore,
    PaintMesh, Size,
};
use windows::{
    Win32::Graphics::{
        Direct3D::{Fxc::*, *},
        Direct3D11::*,
        Dxgi::Common::*,
    },
    core::PCSTR,
};

/// Samples a pixel, as paths are drawn: Direct3D 10.1 guarantees four for
/// these formats.
const SAMPLE_COUNT: u32 = 4;
const COLOR_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;
const DEPTH_FORMAT: DXGI_FORMAT = DXGI_FORMAT_D32_FLOAT;
/// The largest side of a texture at feature level 10.1, the least gpui asks
/// for.
const LARGEST_TARGET: u32 = 8192;
/// The largest buffer Direct3D 11 guarantees, whatever the GPU's memory.
const LARGEST_BUFFER: usize = 128 << 20;

/// A mesh's buffers on the device.
pub(crate) struct Buffers {
    positions: ID3D11Buffer,
    indices: ID3D11Buffer,
    /// Its positions' colors, when it has some.
    colors: Option<ID3D11Buffer>,
    count: u32,
}

/// How the device holds a mesh (`gpui::MeshMaker`).
#[derive(Clone)]
pub(crate) struct Maker(ID3D11Device);

impl MeshMaker for Maker {
    type Buffers = Buffers;
    type Normals = ID3D11Buffer;

    fn holds(&self, mesh: &Mesh) -> bool {
        [
            size_of_val(mesh.positions()),
            size_of_val(mesh.indices()),
            mesh.colors().map_or(0, size_of_val),
        ]
        .into_iter()
        .all(|bytes| bytes <= LARGEST_BUFFER)
    }

    fn buffers(&self, mesh: &Mesh) -> Option<Buffers> {
        Some(Buffers {
            positions: self.buffer(mesh.positions(), D3D11_BIND_VERTEX_BUFFER)?,
            indices: self.buffer(mesh.indices(), D3D11_BIND_INDEX_BUFFER)?,
            colors: match mesh.colors() {
                Some(colors) => Some(self.buffer(colors, D3D11_BIND_VERTEX_BUFFER)?),
                None => None,
            },
            count: mesh.indices().len() as u32,
        })
    }

    fn normals(&self, normals: &[[f32; 3]]) -> Option<ID3D11Buffer> {
        self.buffer(normals, D3D11_BIND_VERTEX_BUFFER)
    }
}

impl Maker {
    /// A buffer holding `data` for good, bound as `bind`.
    fn buffer<T: Copy>(&self, data: &[T], bind: D3D11_BIND_FLAG) -> Option<ID3D11Buffer> {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(size_of_val(data)).ok()?,
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: bind.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr().cast::<c_void>(),
            SysMemPitch: 0,
            SysMemSlicePitch: 0,
        };
        let mut buffer = None;
        // SAFETY: `initial` points at `ByteWidth` readable bytes, copied
        // before the call returns.
        match unsafe {
            self.0
                .CreateBuffer(&desc, Some(&initial), Some(&mut buffer))
        } {
            Ok(()) => buffer,
            Err(error) => {
                log::error!("a mesh's buffer of {} bytes: {error}", desc.ByteWidth);
                None
            }
        }
    }
}

/// The shaders, compiled from naga's translation: the same bytes for every
/// device, so once a process.
struct Compiled {
    vertex: Vec<u8>,
    fragment: Vec<u8>,
    composite_vertex: Vec<u8>,
    composite_fragment: Vec<u8>,
}

impl Compiled {
    fn get() -> Result<&'static Compiled> {
        static COMPILED: OnceLock<Result<Compiled, String>> = OnceLock::new();
        COMPILED
            .get_or_init(|| {
                let compiled =
                    |entry, target| compile(entry, target).map_err(|error| format!("{error:#}"));
                Ok(Compiled {
                    vertex: compiled("vs_mesh\0", "vs_4_1\0")?,
                    fragment: compiled("fs_mesh\0", "ps_4_1\0")?,
                    composite_vertex: compiled("vs_mesh_composite\0", "vs_4_1\0")?,
                    composite_fragment: compiled("fs_mesh_composite\0", "ps_4_1\0")?,
                })
            })
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))
    }
}

/// `entry` of the mesh shaders, compiled for `target`, both nul-terminated.
/// Shader model 4.1, as gpui's other shaders: what feature level 10.1 runs.
fn compile(entry: &str, target: &str) -> Result<Vec<u8>> {
    const SOURCE: &str = include_str!("shaders_mesh.hlsl");
    let mut code = None;
    let mut errors = None;
    // SAFETY: the source is `SOURCE.len()` bytes; `entry` and `target` end
    // with a nul; the blobs are read before they are dropped.
    unsafe {
        let compiled = D3DCompile(
            SOURCE.as_ptr().cast(),
            SOURCE.len(),
            PCSTR(c"shaders_mesh.hlsl".as_ptr().cast()),
            None,
            None,
            PCSTR::from_raw(entry.as_ptr()),
            PCSTR::from_raw(target.as_ptr()),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        );
        if let Err(error) = compiled {
            let said = errors.map(|blob: ID3DBlob| {
                let bytes = slice::from_raw_parts(
                    blob.GetBufferPointer().cast::<u8>(),
                    blob.GetBufferSize(),
                );
                String::from_utf8_lossy(bytes).into_owned()
            });
            anyhow::bail!(
                "{}: {error} {}",
                entry.trim_end_matches('\0'),
                said.unwrap_or_default()
            );
        }
        let code = code.context("no shader code")?;
        Ok(
            slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize())
                .to_vec(),
        )
    }
}

struct Pipelines {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    /// Positions, normals and colors, a buffer each.
    layout: ID3D11InputLayout,
    composite_vertex: ID3D11VertexShader,
    composite_fragment: ID3D11PixelShader,
    depth: ID3D11DepthStencilState,
    /// Either side may face the camera, as a mesh's winding is not always
    /// kept.
    rasterizer: ID3D11RasterizerState,
    /// What is drawn is written as it is: the target holds nothing else.
    drawn: ID3D11BlendState,
    /// What was drawn is premultiplied by its coverage: resolved edges over
    /// nothing.
    composited: ID3D11BlendState,
    params: ID3D11Buffer,
    composite: ID3D11Buffer,
}

impl Pipelines {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let compiled = Compiled::get()?;
        // SAFETY: plain Direct3D calls, every pointer to a value that lives
        // through the call.
        unsafe {
            let mut vertex = None;
            device.CreateVertexShader(&compiled.vertex, None, Some(&mut vertex))?;
            let mut fragment = None;
            device.CreatePixelShader(&compiled.fragment, None, Some(&mut fragment))?;
            let mut composite_vertex = None;
            device.CreateVertexShader(
                &compiled.composite_vertex,
                None,
                Some(&mut composite_vertex),
            )?;
            let mut composite_fragment = None;
            device.CreatePixelShader(
                &compiled.composite_fragment,
                None,
                Some(&mut composite_fragment),
            )?;

            // naga names each location `LOC<n>`.
            let element = |slot: u32, format| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(c"LOC".as_ptr().cast()),
                SemanticIndex: slot,
                Format: format,
                InputSlot: slot,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            };
            let mut layout = None;
            device.CreateInputLayout(
                &[
                    element(0, DXGI_FORMAT_R32G32B32_FLOAT),
                    element(1, DXGI_FORMAT_R32G32B32_FLOAT),
                    element(2, DXGI_FORMAT_R8G8B8A8_UNORM),
                ],
                &compiled.vertex,
                Some(&mut layout),
            )?;

            let mut depth = None;
            device.CreateDepthStencilState(
                &D3D11_DEPTH_STENCIL_DESC {
                    DepthEnable: true.into(),
                    DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
                    DepthFunc: D3D11_COMPARISON_LESS,
                    StencilEnable: false.into(),
                    ..Default::default()
                },
                Some(&mut depth),
            )?;

            let mut rasterizer = None;
            device.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    FrontCounterClockwise: false.into(),
                    DepthBias: 0,
                    DepthBiasClamp: 0.,
                    SlopeScaledDepthBias: 0.,
                    DepthClipEnable: true.into(),
                    ScissorEnable: false.into(),
                    MultisampleEnable: true.into(),
                    AntialiasedLineEnable: false.into(),
                },
                Some(&mut rasterizer),
            )?;

            let mut desc = D3D11_BLEND_DESC::default();
            desc.RenderTarget[0].BlendEnable = false.into();
            desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
            let mut drawn = None;
            device.CreateBlendState(&desc, Some(&mut drawn))?;
            let target = &mut desc.RenderTarget[0];
            target.BlendEnable = true.into();
            target.BlendOp = D3D11_BLEND_OP_ADD;
            target.BlendOpAlpha = D3D11_BLEND_OP_ADD;
            target.SrcBlend = D3D11_BLEND_ONE;
            target.SrcBlendAlpha = D3D11_BLEND_ONE;
            target.DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
            target.DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
            let mut composited = None;
            device.CreateBlendState(&desc, Some(&mut composited))?;

            let constants = |bytes: usize| -> Result<ID3D11Buffer> {
                let mut buffer = None;
                device.CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: bytes as u32,
                        Usage: D3D11_USAGE_DYNAMIC,
                        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                        MiscFlags: 0,
                        StructureByteStride: 0,
                    },
                    None,
                    Some(&mut buffer),
                )?;
                buffer.context("no constant buffer")
            };

            Ok(Pipelines {
                vertex: vertex.context("no vertex shader")?,
                fragment: fragment.context("no pixel shader")?,
                layout: layout.context("no input layout")?,
                composite_vertex: composite_vertex.context("no vertex shader")?,
                composite_fragment: composite_fragment.context("no pixel shader")?,
                depth: depth.context("no depth state")?,
                rasterizer: rasterizer.context("no rasterizer state")?,
                drawn: drawn.context("no blend state")?,
                composited: composited.context("no blend state")?,
                params: constants(size_of::<MeshParams>())?,
                composite: constants(size_of::<MeshComposite>())?,
            })
        }
    }
}

/// Where meshes are drawn: a multisampled colour and depth, resolved into a
/// texture the composite reads.
struct Target {
    width: u32,
    height: u32,
    multisampled: ID3D11Texture2D,
    multisampled_view: ID3D11RenderTargetView,
    depth_view: ID3D11DepthStencilView,
    resolved: ID3D11Texture2D,
    resolved_view: ID3D11ShaderResourceView,
}

impl Target {
    fn new(device: &ID3D11Device, width: u32, height: u32) -> Result<Self> {
        let texture = |format, samples, bind: i32| -> Result<ID3D11Texture2D> {
            let mut texture = None;
            // SAFETY: the description lives through the call.
            unsafe {
                device.CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: width.max(1),
                        Height: height.max(1),
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: format,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: samples,
                            Quality: if samples > 1 {
                                D3D11_STANDARD_MULTISAMPLE_PATTERN.0 as u32
                            } else {
                                0
                            },
                        },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: bind as u32,
                        CPUAccessFlags: 0,
                        MiscFlags: 0,
                    },
                    None,
                    Some(&mut texture),
                )?
            };
            texture.context("no texture")
        };
        let multisampled = texture(COLOR_FORMAT, SAMPLE_COUNT, D3D11_BIND_RENDER_TARGET.0)?;
        let depth = texture(DEPTH_FORMAT, SAMPLE_COUNT, D3D11_BIND_DEPTH_STENCIL.0)?;
        let resolved = texture(COLOR_FORMAT, 1, D3D11_BIND_SHADER_RESOURCE.0)?;
        let (mut multisampled_view, mut depth_view, mut resolved_view) = (None, None, None);
        // SAFETY: views of the textures just made.
        unsafe {
            device.CreateRenderTargetView(&multisampled, None, Some(&mut multisampled_view))?;
            device.CreateDepthStencilView(&depth, None, Some(&mut depth_view))?;
            device.CreateShaderResourceView(&resolved, None, Some(&mut resolved_view))?;
        }
        Ok(Target {
            width,
            height,
            multisampled,
            multisampled_view: multisampled_view.context("no render target view")?,
            depth_view: depth_view.context("no depth view")?,
            resolved,
            resolved_view: resolved_view.context("no shader resource view")?,
        })
    }
}

pub(crate) struct MeshPass {
    device: ID3D11Device,
    /// Made when the first mesh is asked for; `None` if they cannot be.
    pipelines: OnceCell<Option<Pipelines>>,
    meshes: MeshStore<Maker>,
    target: Option<Target>,
}

impl MeshPass {
    pub(crate) fn new(device: &ID3D11Device) -> Self {
        Self {
            device: device.clone(),
            pipelines: OnceCell::new(),
            meshes: MeshStore::new(Maker(device.clone())),
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

    /// Draw `paint` into the target, then composite it into the frame
    /// `frame` draws into, of `viewport`'s size. Leaves the device context
    /// set for meshes, but for its rasterizer state: the caller sets the
    /// rest back for its own primitives.
    pub(crate) fn draw(
        &mut self,
        context: &ID3D11DeviceContext,
        paint: &PaintMesh,
        frame: &Option<ID3D11RenderTargetView>,
        viewport: &D3D11_VIEWPORT,
    ) -> Result<()> {
        let size = Size {
            width: DevicePixels(viewport.Width as i32),
            height: DevicePixels(viewport.Height as i32),
        };
        let Some(framed) = paint.framed(size, LARGEST_TARGET) else {
            return Ok(());
        };
        let Some((buffers, normals)) = self.meshes.drawn(paint.mesh.id()) else {
            return Ok(());
        };
        let [width, height] = framed.target;
        if self
            .target
            .as_ref()
            .is_none_or(|target| (target.width, target.height) != (width, height))
        {
            self.target = None;
            self.target = Some(Target::new(&self.device, width, height)?);
        }
        let (Some(Some(pipelines)), Some(target)) = (self.pipelines.get(), self.target.as_ref())
        else {
            return Ok(());
        };
        // Flat until its normals are made.
        let smooth = paint.view.smooth && normals.is_some();
        let colored = buffers.colors.is_some();
        write(
            context,
            &pipelines.params,
            &framed.params(&paint.view, smooth, colored),
        )?;
        write(context, &pipelines.composite, &framed.composite())?;
        // Without normals or colors the shader reads none: the positions
        // stand in, as long as either would be.
        let vertices = [
            Some(buffers.positions.clone()),
            Some(normals.unwrap_or(&buffers.positions).clone()),
            Some(
                buffers
                    .colors
                    .as_ref()
                    .unwrap_or(&buffers.positions)
                    .clone(),
            ),
        ];
        let strides = [12, 12, 4];
        let offsets = [0; 3];
        let [shown_width, shown_height] = framed.shown;
        // SAFETY: plain Direct3D calls, every pointer to a value that lives
        // through the call.
        unsafe {
            let rasterizer = context.RSGetState().ok();
            context.RSSetState(&pipelines.rasterizer);
            context.ClearRenderTargetView(&target.multisampled_view, &[0.; 4]);
            context.ClearDepthStencilView(&target.depth_view, D3D11_CLEAR_DEPTH.0 as u32, 1., 0);
            context.OMSetRenderTargets(
                Some(&[Some(target.multisampled_view.clone())]),
                &target.depth_view,
            );
            context.OMSetDepthStencilState(&pipelines.depth, 0);
            context.OMSetBlendState(&pipelines.drawn, None, 0xFFFFFFFF);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.,
                TopLeftY: 0.,
                Width: shown_width as f32,
                Height: shown_height as f32,
                MinDepth: 0.,
                MaxDepth: 1.,
            }]));
            context.IASetInputLayout(&pipelines.layout);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.IASetVertexBuffers(
                0,
                3,
                Some(vertices.as_ptr()),
                Some(strides.as_ptr()),
                Some(offsets.as_ptr()),
            );
            context.IASetIndexBuffer(&buffers.indices, DXGI_FORMAT_R32_UINT, 0);
            context.VSSetShader(&pipelines.vertex, None);
            context.PSSetShader(&pipelines.fragment, None);
            let params = [Some(pipelines.params.clone())];
            context.VSSetConstantBuffers(0, Some(&params));
            context.PSSetConstantBuffers(0, Some(&params));
            context.DrawIndexed(buffers.count, 0, 0);
            context.ResolveSubresource(&target.resolved, 0, &target.multisampled, 0, COLOR_FORMAT);

            // Composited where it is shown, pixel for pixel.
            context.OMSetRenderTargets(Some(slice::from_ref(frame)), None);
            context.OMSetDepthStencilState(None, 0);
            context.OMSetBlendState(&pipelines.composited, None, 0xFFFFFFFF);
            context.RSSetViewports(Some(slice::from_ref(viewport)));
            // Let go of the mesh's buffers.
            context.IASetInputLayout(None);
            context.IASetVertexBuffers(
                0,
                3,
                Some([None, None, None].as_ptr()),
                Some(offsets.as_ptr()),
                Some(offsets.as_ptr()),
            );
            context.IASetIndexBuffer(None, DXGI_FORMAT_R32_UINT, 0);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            context.VSSetShader(&pipelines.composite_vertex, None);
            context.PSSetShader(&pipelines.composite_fragment, None);
            let composite = [Some(pipelines.composite.clone())];
            context.VSSetConstantBuffers(1, Some(&composite));
            context.PSSetConstantBuffers(1, Some(&composite));
            context.PSSetShaderResources(0, Some(&[Some(target.resolved_view.clone())]));
            context.Draw(4, 0);
            // Free to be drawn into again.
            context.PSSetShaderResources(0, Some(&[None]));
            context.RSSetState(rasterizer.as_ref());
        }
        Ok(())
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

/// Write `value` into a dynamic constant buffer.
fn write<T: Copy>(context: &ID3D11DeviceContext, buffer: &ID3D11Buffer, value: &T) -> Result<()> {
    // SAFETY: the buffer was made `size_of::<T>()` bytes long, and is
    // written whole while mapped.
    unsafe {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(value, mapped.pData.cast::<T>(), 1);
        context.Unmap(buffer, 0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, ContentMask, MeshLight, MeshView, ScaledPixels, point, size};
    use windows::Win32::Foundation::HMODULE;

    const SIDE: u32 = 64;

    fn device() -> (ID3D11Device, ID3D11DeviceContext) {
        for driver in [D3D_DRIVER_TYPE_WARP, D3D_DRIVER_TYPE_HARDWARE] {
            let (mut device, mut context) = (None, None);
            let made = unsafe {
                D3D11CreateDevice(
                    None,
                    driver,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&[D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1]),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )
            };
            if let (Ok(()), Some(device), Some(context)) = (made, device, context) {
                return (device, context);
            }
        }
        panic!("no Direct3D 11 device");
    }

    /// A frame of `SIDE` pixels a side, black, and the view drawn into it.
    fn frame(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
    ) -> (ID3D11Texture2D, ID3D11RenderTargetView) {
        let mut texture = None;
        let mut view = None;
        unsafe {
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: SIDE,
                        Height: SIDE,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: COLOR_FORMAT,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                        CPUAccessFlags: 0,
                        MiscFlags: 0,
                    },
                    None,
                    Some(&mut texture),
                )
                .unwrap();
            let texture = texture.unwrap();
            device
                .CreateRenderTargetView(&texture, None, Some(&mut view))
                .unwrap();
            let view = view.unwrap();
            context.ClearRenderTargetView(&view, &[0., 0., 0., 1.]);
            (texture, view)
        }
    }

    /// The frame's pixels, red, green, blue, alpha, row after row.
    fn pixels(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        texture: &ID3D11Texture2D,
    ) -> Vec<[u8; 4]> {
        let mut staging = None;
        unsafe {
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: SIDE,
                        Height: SIDE,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: COLOR_FORMAT,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_STAGING,
                        BindFlags: 0,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        MiscFlags: 0,
                    },
                    None,
                    Some(&mut staging),
                )
                .unwrap();
            let staging = staging.unwrap();
            context.CopyResource(&staging, texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();
            let mut pixels = Vec::new();
            for row in 0..SIDE as usize {
                let bytes = slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(row * mapped.RowPitch as usize),
                    SIDE as usize * 4,
                );
                pixels.extend(
                    bytes
                        .chunks_exact(4)
                        .map(|bgra| [bgra[2], bgra[1], bgra[0], bgra[3]]),
                );
            }
            context.Unmap(&staging, 0);
            pixels
        }
    }

    /// Two squares in their own colours, the blue one nearer though drawn
    /// first: where they overlap, it shows. Drawn in the middle half of the
    /// frame, composited over black.
    #[test]
    fn draws_a_mesh_in_depth_where_it_is() {
        let (device, context) = device();
        let square = |left: f32, right: f32, z: f32| {
            [
                [left, -0.5, z],
                [right, -0.5, z],
                [right, 0.5, z],
                [left, 0.5, z],
            ]
        };
        let positions = [square(0., 0.5, 0.25), square(-0.5, 0.5, 0.5)].concat();
        let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let colors = [[[0, 0, 255, 255]; 4], [[255, 0, 0, 255]; 4]].concat();
        let mesh = Arc::new(
            Mesh::new(positions, indices)
                .unwrap()
                .with_colors(colors)
                .unwrap(),
        );
        let mut pass = MeshPass::new(&device);
        match pass.prepare_mesh(&mesh, false) {
            MeshPreparation::Started(uploaded) => futures::executor::block_on(uploaded).unwrap(),
            _ => panic!("the upload should start"),
        }
        assert!(matches!(
            pass.prepare_mesh(&mesh, false),
            MeshPreparation::Ready
        ));

        let identity = [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ];
        let light = MeshLight {
            direction: [0., 0., 1.],
            strength: 0.,
        };
        let pixels_at = |x: f32, y: f32, width: f32, height: f32| Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(y)),
            size: size(ScaledPixels(width), ScaledPixels(height)),
        };
        let quarter = SIDE as f32 / 4.;
        let paint = PaintMesh {
            order: 0,
            bounds: pixels_at(quarter, quarter, 2. * quarter, 2. * quarter),
            content_mask: ContentMask {
                bounds: pixels_at(0., 0., SIDE as f32, SIDE as f32),
            },
            mesh: Arc::clone(&mesh),
            view: MeshView {
                model_view: identity,
                projection: identity,
                perspective: false,
                smooth: false,
                color: [0.5; 3],
                ambient: 1.,
                lights: [light, light],
            },
        };
        let (texture, view) = frame(&device, &context);
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.,
            TopLeftY: 0.,
            Width: SIDE as f32,
            Height: SIDE as f32,
            MinDepth: 0.,
            MaxDepth: 1.,
        };
        pass.draw(&context, &paint, &Some(view), &viewport).unwrap();
        pass.end_frame();

        let pixels = pixels(&device, &context, &texture);
        let at = |x: u32, y: u32| pixels[(y * SIDE + x) as usize];
        let middle = SIDE / 2;
        // The mesh spans the middle quarter of the frame: its left half
        // red, its right half blue, in front.
        assert_eq!(at(middle - 4, middle), [255, 0, 0, 255]);
        assert_eq!(at(middle + 4, middle), [0, 0, 255, 255]);
        // Around it, in its bounds and out of them, the frame as it was.
        assert_eq!(at(middle, SIDE / 4 + 2), [0, 0, 0, 255]);
        assert_eq!(at(2, 2), [0, 0, 0, 255]);
        // Let go of once a frame does not draw it.
        pass.end_frame();
        assert!(pass.meshes.is_empty() && pass.target.is_none());
    }

    /// Whether this runs under Wine, whose HLSL compiler (vkd3d) cannot
    /// compile gpui's own shaders: no renderer can be made there.
    fn under_wine() -> bool {
        use windows::{
            Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
            core::{s, w},
        };
        unsafe { GetModuleHandleW(w!("ntdll.dll")) }
            .ok()
            .and_then(|ntdll| unsafe { GetProcAddress(ntdll, s!("wine_get_version")) })
            .is_some()
    }

    /// Among other primitives, in the renderer: a quad painted before the
    /// mesh shows around it, one painted after covers it, each where it is.
    #[test]
    fn the_renderer_draws_a_mesh_among_other_primitives() {
        if under_wine() {
            eprintln!("skipped: Wine cannot compile gpui's shaders");
            return;
        }
        use gpui::{
            BorderStyle, Corners, Edges, Hsla, Quad, Scene, WindowBackgroundAppearance, hsla,
        };
        use windows::{
            Win32::UI::WindowsAndMessaging::{CreateWindowExW, WINDOW_EX_STYLE, WS_POPUP},
            core::w,
        };

        let devices = crate::DirectXDevices::new().expect("a device");
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                SIDE as i32,
                SIDE as i32,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a window");
        let mut renderer = crate::DirectXRenderer::new(hwnd, &devices, true).expect("a renderer");
        renderer
            .resize(size(DevicePixels(SIDE as i32), DevicePixels(SIDE as i32)))
            .unwrap();

        // A square facing the camera, half the bounds wide and high.
        let mesh = Arc::new(
            Mesh::new(
                vec![
                    [-0.5, -0.5, 0.5],
                    [0.5, -0.5, 0.5],
                    [0.5, 0.5, 0.5],
                    [-0.5, 0.5, 0.5],
                ],
                vec![0, 1, 2, 0, 2, 3],
            )
            .unwrap()
            .with_colors(vec![[0, 0, 255, 255]; 4])
            .unwrap(),
        );
        loop {
            match renderer.prepare_mesh(&mesh, false) {
                MeshPreparation::Ready => break,
                MeshPreparation::Started(uploaded) => {
                    futures::executor::block_on(uploaded).expect("uploaded")
                }
                MeshPreparation::Pending | MeshPreparation::Flat(_) => std::thread::yield_now(),
                MeshPreparation::Never => panic!("never drawn"),
            }
        }

        let bounds = |x: f32, y: f32, width: f32, height: f32| Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(y)),
            size: size(ScaledPixels(width), ScaledPixels(height)),
        };
        let quad = |at: Bounds<ScaledPixels>, color: Hsla| Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds: at,
            content_mask: ContentMask { bounds: at },
            background: color.into(),
            border_color: color,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
        };
        let identity = [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ];
        let light = MeshLight {
            direction: [0., 0., 1.],
            strength: 0.,
        };
        let quarter = SIDE as f32 / 4.;
        let mut scene = Scene::default();
        // Green under the mesh's bounds, red over its right side.
        scene.insert_primitive(quad(
            bounds(0., 0., SIDE as f32, SIDE as f32 / 2.),
            hsla(1. / 3., 1., 0.5, 1.),
        ));
        scene.insert_primitive(PaintMesh {
            order: 0,
            bounds: bounds(quarter, quarter, 2. * quarter, 2. * quarter),
            content_mask: ContentMask {
                bounds: bounds(0., 0., SIDE as f32, SIDE as f32),
            },
            mesh: Arc::clone(&mesh),
            view: MeshView {
                model_view: identity,
                projection: identity,
                perspective: false,
                smooth: false,
                color: [0.5; 3],
                ambient: 1.,
                lights: [light, light],
            },
        });
        scene.insert_primitive(quad(
            bounds(36., 0., 4., SIDE as f32),
            hsla(0., 1., 0.5, 1.),
        ));
        scene.finish();
        let image = renderer
            .render_to_image(&scene, WindowBackgroundAppearance::Opaque)
            .unwrap();
        let at = |x: u32, y: u32| image.get_pixel(x, y).0;
        let middle = SIDE / 2;
        assert_eq!(at(middle, middle), [0, 0, 255, 255], "the mesh");
        assert_eq!(at(37, middle), [255, 0, 0, 255], "the quad after it");
        assert_eq!(
            at(middle, 20),
            [0, 255, 0, 255],
            "the quad under its bounds"
        );
        assert_eq!(
            at(middle, 44),
            [255, 255, 255, 255],
            "the window's background"
        );
    }
}
