// Made by naga from gpui_wgpu's shaders_mesh.wgsl: do not edit. Its
// test `the_mesh_shader_s_translations_are_current` makes it again.
struct MeshParams {
    row_major float4x4 model_view;
    row_major float4x4 projection;
    float2 ndc_scale;
    float2 ndc_offset;
    float4 color;
    float4 light0_;
    float4 light1_;
    float ambient;
    uint perspective;
    uint shaded_smooth;
    uint colored;
};

struct MeshVarying {
    float4 position : SV_Position;
    float3 eye : LOC0;
    float3 normal : LOC1;
    float3 color : LOC2;
};

struct MeshComposite {
    float2 origin;
    float2 size;
    float2 viewport;
    float2 pad;
};

cbuffer mesh : register(b0) { MeshParams mesh; }
cbuffer composite : register(b1) { MeshComposite composite; }
Texture2D<float4> t_mesh : register(t0);

struct VertexOutput_vs_mesh {
    float3 eye : LOC0;
    float3 normal_2 : LOC1;
    float3 color_1 : LOC2;
    float4 position_2 : SV_Position;
};

struct FragmentInput_fs_mesh {
    float3 eye_1 : LOC0;
    float3 normal_3 : LOC1;
    float3 color_2 : LOC2;
    float4 position_3 : SV_Position;
};

struct FragmentInput_fs_mesh_composite {
    float4 position_4 : SV_Position;
};

float3 linear_from_srgb(float3 srgb)
{
    float3 low = (srgb / (12.92).xxx);
    float3 high = pow(((srgb + (0.055).xxx) / (1.055).xxx), (2.4).xxx);
    return ((srgb <= (0.04045).xxx) ? low : high);
}

float3 srgb_from_linear(float3 linear_)
{
    float3 v = clamp(linear_, (0.0).xxx, (1.0).xxx);
    float3 low_1 = (v * 12.92);
    float3 high_1 = ((1.055 * pow(v, (0.41666666).xxx)) - (0.055).xxx);
    return ((v <= (0.0031308).xxx) ? low_1 : high_1);
}

VertexOutput_vs_mesh vs_mesh(float3 position : LOC0, float3 normal : LOC1, float4 color : LOC2)
{
    MeshVarying out_ = (MeshVarying)0;

    float4x4 _e5 = mesh.model_view;
    float4 eye_2 = mul(float4(position, 1.0), _e5);
    float4x4 _e11 = mesh.projection;
    float4 clip_ = mul(eye_2, _e11);
    float2 _e18 = mesh.ndc_scale;
    float2 _e22 = mesh.ndc_offset;
    out_.position = float4(((clip_.xy * _e18) + (_e22 * clip_.w)), clip_.z, clip_.w);
    out_.eye = eye_2.xyz;
    float4x4 _e34 = mesh.model_view;
    out_.normal = mul(float4(normal, 0.0), _e34).xyz;
    float4 _e42 = mesh.color;
    out_.color = _e42.xyz;
    uint _e46 = mesh.colored;
    if ((_e46 != 0u)) {
        const float3 _e51 = linear_from_srgb(color.xyz);
        out_.color = _e51;
    }
    MeshVarying _e52 = out_;
    const MeshVarying meshvarying = _e52;
    const VertexOutput_vs_mesh meshvarying_1 = { meshvarying.eye, meshvarying.normal, meshvarying.color, meshvarying.position };
    return meshvarying_1;
}

float4 fs_mesh(FragmentInput_fs_mesh fragmentinput_fs_mesh) : SV_Target0
{
    MeshVarying input = { fragmentinput_fs_mesh.position_3, fragmentinput_fs_mesh.eye_1, fragmentinput_fs_mesh.normal_3, fragmentinput_fs_mesh.color_2 };
    float3 toward = float3(0.0, 0.0, 1.0);
    float3 normal_1 = (float3)0;
    bool local = (bool)0;
    float3 surface = (float3)0;

    uint _e8 = mesh.perspective;
    if ((_e8 != 0u)) {
        toward = normalize(-(input.eye));
    }
    float3 _e15 = ddx(input.eye);
    float3 _e17 = ddy(input.eye);
    float3 across = cross(_e15, _e17);
    float3 _e19 = toward;
    normal_1 = _e19;
    float length_squared = dot(across, across);
    if ((length_squared > 1e-30)) {
        normal_1 = (across * rsqrt(length_squared));
    }
    float3 _e26 = normal_1;
    float3 _e27 = toward;
    if ((dot(_e26, _e27) < 0.0)) {
        float3 _e31 = normal_1;
        normal_1 = -(_e31);
    }
    float smooth_length = dot(input.normal, input.normal);
    uint _e38 = mesh.shaded_smooth;
    if ((_e38 != 0u)) {
        local = (smooth_length > 1e-30);
    } else {
        local = false;
    }
    bool _e46 = local;
    if (_e46) {
        surface = (input.normal * rsqrt(smooth_length));
        float3 _e51 = surface;
        float3 _e52 = normal_1;
        if ((dot(_e51, _e52) < 0.0)) {
            float3 _e56 = surface;
            surface = -(_e56);
        }
        float3 _e58 = surface;
        normal_1 = _e58;
    }
    float _e61 = mesh.ambient;
    float _e65 = mesh.light0_.w;
    float3 _e66 = normal_1;
    float4 _e69 = mesh.light0_;
    float _e79 = mesh.light1_.w;
    float3 _e80 = normal_1;
    float4 _e83 = mesh.light1_;
    float light = ((_e61 + (_e65 * max(dot(_e66, _e69.xyz), 0.0))) + (_e79 * max(dot(_e80, _e83.xyz), 0.0)));
    const float3 _e92 = srgb_from_linear((input.color * light));
    return float4(_e92, 1.0);
}

float4 vs_mesh_composite(uint vertex_id : SV_VertexID) : SV_Position
{
    float2 unit = float2(float((vertex_id & 1u)), (0.5 * float((vertex_id & 2u))));
    float2 _e12 = composite.origin;
    float2 _e15 = composite.size;
    float2 pixel = (_e12 + (unit * _e15));
    float2 _e20 = composite.viewport;
    float2 device = (((pixel / _e20) * float2(2.0, -2.0)) + float2(-1.0, 1.0));
    return float4(device, 0.0, 1.0);
}

int2 naga_f2i32(float2 value) {
    return int2(clamp(value, -2147483600.0, 2147483500.0));
}

float4 fs_mesh_composite(FragmentInput_fs_mesh_composite fragmentinput_fs_mesh_composite) : SV_Target0
{
    float4 position_1 = fragmentinput_fs_mesh_composite.position_4;
    float2 _e4 = composite.origin;
    int2 texel = naga_f2i32(floor((position_1.xy - _e4)));
    float4 _e10 = t_mesh.Load(int3(texel, int(0)));
    return _e10;
}
