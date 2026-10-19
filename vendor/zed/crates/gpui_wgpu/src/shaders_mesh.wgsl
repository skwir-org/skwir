// Meshes drawn in 3D (`gpui::PaintMesh`): each into an offscreen target, with
// depth, then composited into the frame. A module of its own, made when the
// first mesh is drawn.

struct MeshParams {
    model_view: mat4x4<f32>,
    projection: mat4x4<f32>,
    // Clip space spans the mesh's bounds; the target's viewport spans the part
    // of them shown: x and y are scaled and moved from the one to the other.
    ndc_scale: vec2<f32>,
    ndc_offset: vec2<f32>,
    color: vec4<f32>,
    // Toward each light in the camera's coordinates, its strength in w.
    light0: vec4<f32>,
    light1: vec4<f32>,
    ambient: f32,
    perspective: u32,
    pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> mesh: MeshParams;

struct MeshVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) eye: vec3<f32>,
}

@vertex
fn vs_mesh(@location(0) position: vec3<f32>) -> MeshVarying {
    let eye = mesh.model_view * vec4<f32>(position, 1.0);
    let clip = mesh.projection * eye;
    var out: MeshVarying;
    out.position = vec4<f32>(
        clip.xy * mesh.ndc_scale + mesh.ndc_offset * clip.w,
        clip.z,
        clip.w,
    );
    out.eye = eye.xyz;
    return out;
}

fn srgb_from_linear(linear: vec3<f32>) -> vec3<f32> {
    let v = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = v * 12.92;
    let high = 1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, v <= vec3<f32>(0.0031308));
}

@fragment
fn fs_mesh(input: MeshVarying) -> @location(0) vec4<f32> {
    // Toward the camera: its position with perspective, straight ahead without.
    var toward = vec3<f32>(0.0, 0.0, 1.0);
    if (mesh.perspective != 0u) {
        toward = normalize(-input.eye);
    }
    // The face's own normal: the position's derivatives across the screen
    // span its plane, so no normals are sent. Either side may face the
    // camera, as a mesh's winding is not always kept.
    let across = cross(dpdx(input.eye), dpdy(input.eye));
    var normal = toward;
    let length_squared = dot(across, across);
    if (length_squared > 1e-30) {
        normal = across * inverseSqrt(length_squared);
    }
    if (dot(normal, toward) < 0.0) {
        normal = -normal;
    }
    let light = mesh.ambient
        + mesh.light0.w * max(dot(normal, mesh.light0.xyz), 0.0)
        + mesh.light1.w * max(dot(normal, mesh.light1.xyz), 0.0);
    return vec4<f32>(srgb_from_linear(mesh.color.rgb * light), 1.0);
}

// --- the target composited into the frame --- //

struct MeshComposite {
    // The part of the bounds shown, in the frame's pixels.
    origin: vec2<f32>,
    size: vec2<f32>,
    viewport: vec2<f32>,
    pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> composite: MeshComposite;
@group(0) @binding(1) var t_mesh: texture_2d<f32>;

@vertex
fn vs_mesh_composite(@builtin(vertex_index) vertex_id: u32) -> @builtin(position) vec4<f32> {
    let unit = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let pixel = composite.origin + unit * composite.size;
    let device = pixel / composite.viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    return vec4<f32>(device, 0.0, 1.0);
}

@fragment
fn fs_mesh_composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    // Pixel for pixel: the target's top left is the shown part's.
    let texel = vec2<i32>(floor(position.xy - composite.origin));
    return textureLoad(t_mesh, texel, 0);
}
