// Made by naga from gpui_wgpu's shaders_mesh.wgsl: do not edit. Its
// test `the_mesh_shader_s_translations_are_current` makes it again.
// language: metal2.0
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct MeshParams {
    metal::float4x4 model_view;
    metal::float4x4 projection;
    metal::float2 ndc_scale;
    metal::float2 ndc_offset;
    metal::float4 color;
    metal::float4 light0_;
    metal::float4 light1_;
    float ambient;
    uint perspective;
    uint shaded_smooth;
    uint colored;
};
struct MeshVarying {
    metal::float4 position;
    metal::float3 eye;
    metal::float3 normal;
    metal::float3 color;
};
struct MeshComposite {
    metal::float2 origin;
    metal::float2 size;
    metal::float2 viewport;
    metal::float2 pad;
};

metal::float3 linear_from_srgb(
    metal::float3 srgb
) {
    metal::float3 low = srgb / metal::float3(12.92);
    metal::float3 high = metal::pow((srgb + metal::float3(0.055)) / metal::float3(1.055), metal::float3(2.4));
    return metal::select(high, low, srgb <= metal::float3(0.04045));
}

metal::float3 srgb_from_linear(
    metal::float3 linear
) {
    metal::float3 v = metal::clamp(linear, metal::float3(0.0), metal::float3(1.0));
    metal::float3 low_1 = v * 12.92;
    metal::float3 high_1 = (1.055 * metal::pow(v, metal::float3(0.41666666))) - metal::float3(0.055);
    return metal::select(high_1, low_1, v <= metal::float3(0.0031308));
}

struct vs_meshInput {
    metal::float3 position [[attribute(0)]];
    metal::float3 normal [[attribute(1)]];
    metal::float4 color [[attribute(2)]];
};
struct vs_meshOutput {
    metal::float4 position [[position]];
    metal::float3 eye [[user(loc0), center_perspective]];
    metal::float3 normal [[user(loc1), center_perspective]];
    metal::float3 color [[user(loc2), center_perspective]];
};
vertex vs_meshOutput vs_mesh(
  vs_meshInput varyings [[stage_in]]
, constant MeshParams& mesh [[buffer(0)]]
) {
    const auto position = varyings.position;
    const auto normal = varyings.normal;
    const auto color = varyings.color;
    MeshVarying out = {};
    metal::float4x4 _e5 = mesh.model_view;
    metal::float4 eye = _e5 * metal::float4(position, 1.0);
    metal::float4x4 _e11 = mesh.projection;
    metal::float4 clip = _e11 * eye;
    metal::float2 _e18 = mesh.ndc_scale;
    metal::float2 _e22 = mesh.ndc_offset;
    out.position = metal::float4((clip.xy * _e18) + (_e22 * clip.w), clip.z, clip.w);
    out.eye = eye.xyz;
    metal::float4x4 _e34 = mesh.model_view;
    out.normal = (_e34 * metal::float4(normal, 0.0)).xyz;
    metal::float4 _e42 = mesh.color;
    out.color = _e42.xyz;
    uint _e46 = mesh.colored;
    if (_e46 != 0u) {
        metal::float3 _e51 = linear_from_srgb(color.xyz);
        out.color = _e51;
    }
    MeshVarying _e52 = out;
    const auto _tmp = _e52;
    return vs_meshOutput { _tmp.position, _tmp.eye, _tmp.normal, _tmp.color };
}


struct fs_meshInput {
    metal::float3 eye [[user(loc0), center_perspective]];
    metal::float3 normal_2 [[user(loc1), center_perspective]];
    metal::float3 color_1 [[user(loc2), center_perspective]];
};
struct fs_meshOutput {
    metal::float4 member_1 [[color(0)]];
};
fragment fs_meshOutput fs_mesh(
  fs_meshInput varyings_1 [[stage_in]]
, metal::float4 position_2 [[position]]
, constant MeshParams& mesh [[buffer(0)]]
) {
    const MeshVarying input = { position_2, varyings_1.eye, varyings_1.normal_2, varyings_1.color_1 };
    metal::float3 toward = metal::float3(0.0, 0.0, 1.0);
    metal::float3 normal_1 = {};
    bool local = {};
    metal::float3 surface = {};
    uint _e8 = mesh.perspective;
    if (_e8 != 0u) {
        toward = metal::normalize(-(input.eye));
    }
    metal::float3 _e15 = metal::dfdx(input.eye);
    metal::float3 _e17 = metal::dfdy(input.eye);
    metal::float3 across = metal::cross(_e15, _e17);
    metal::float3 _e19 = toward;
    normal_1 = _e19;
    float length_squared = metal::dot(across, across);
    if (length_squared > 0.000000000000000000000000000001) {
        normal_1 = across * metal::rsqrt(length_squared);
    }
    metal::float3 _e26 = normal_1;
    metal::float3 _e27 = toward;
    if (metal::dot(_e26, _e27) < 0.0) {
        metal::float3 _e31 = normal_1;
        normal_1 = -(_e31);
    }
    float smooth_length = metal::dot(input.normal, input.normal);
    uint _e38 = mesh.shaded_smooth;
    if (_e38 != 0u) {
        local = smooth_length > 0.000000000000000000000000000001;
    } else {
        local = false;
    }
    bool _e46 = local;
    if (_e46) {
        surface = input.normal * metal::rsqrt(smooth_length);
        metal::float3 _e51 = surface;
        metal::float3 _e52 = normal_1;
        if (metal::dot(_e51, _e52) < 0.0) {
            metal::float3 _e56 = surface;
            surface = -(_e56);
        }
        metal::float3 _e58 = surface;
        normal_1 = _e58;
    }
    float _e61 = mesh.ambient;
    float _e65 = mesh.light0_.w;
    metal::float3 _e66 = normal_1;
    metal::float4 _e69 = mesh.light0_;
    float _e79 = mesh.light1_.w;
    metal::float3 _e80 = normal_1;
    metal::float4 _e83 = mesh.light1_;
    float light = (_e61 + (_e65 * metal::max(metal::dot(_e66, _e69.xyz), 0.0))) + (_e79 * metal::max(metal::dot(_e80, _e83.xyz), 0.0));
    metal::float3 _e92 = srgb_from_linear(input.color * light);
    return fs_meshOutput { metal::float4(_e92, 1.0) };
}


struct vs_mesh_compositeInput {
};
struct vs_mesh_compositeOutput {
    metal::float4 member_2 [[position]];
};
vertex vs_mesh_compositeOutput vs_mesh_composite(
  uint vertex_id [[vertex_id]]
, constant MeshComposite& composite [[buffer(0)]]
) {
    metal::float2 unit = metal::float2(static_cast<float>(vertex_id & 1u), 0.5 * static_cast<float>(vertex_id & 2u));
    metal::float2 _e12 = composite.origin;
    metal::float2 _e15 = composite.size;
    metal::float2 pixel = _e12 + (unit * _e15);
    metal::float2 _e20 = composite.viewport;
    metal::float2 device_ = ((pixel / _e20) * metal::float2(2.0, -2.0)) + metal::float2(-1.0, 1.0);
    return vs_mesh_compositeOutput { metal::float4(device_, 0.0, 1.0) };
}

metal::int2 naga_f2i32(metal::float2 value) {
    return static_cast<metal::int2>(metal::clamp(value, -2147483600.0, 2147483500.0));
}


struct fs_mesh_compositeInput {
};
struct fs_mesh_compositeOutput {
    metal::float4 member_3 [[color(0)]];
};
fragment fs_mesh_compositeOutput fs_mesh_composite(
  metal::float4 position_1 [[position]]
, constant MeshComposite& composite [[buffer(0)]]
, metal::texture2d<float, metal::access::sample> t_mesh [[texture(0)]]
) {
    metal::float2 _e4 = composite.origin;
    metal::int2 texel = naga_f2i32(metal::floor(position_1.xy - _e4));
    metal::float4 _e10 = t_mesh.read(metal::uint2(texel), 0);
    return fs_mesh_compositeOutput { _e10 };
}
