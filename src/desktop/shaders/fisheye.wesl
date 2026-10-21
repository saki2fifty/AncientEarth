// Original MIT shader. Equidistant optical projection from six perspective faces.
import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
struct Settings { parameters: vec4<f32> };
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var front: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var front_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var right: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var right_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var back: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var back_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var left: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var left_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var up: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var up_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var down: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(12) var down_sampler: sampler;
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let xy = vec2((2.0 * in.uv.x - 1.0) * settings.parameters.x, 1.0 - 2.0 * in.uv.y);
    let radius = length(xy);
    if radius > 1.0 { return vec4(0.002, 0.004, 0.008, 1.0); }
    let theta = radius * settings.parameters.y;
    let scale = select(settings.parameters.y, sin(theta) / max(radius, 1e-8), radius > 1e-8);
    let ray = vec3(xy * scale, cos(theta));
    let axis = abs(ray);
    if axis.z >= axis.x && axis.z >= axis.y {
        if ray.z >= 0.0 {
            let uv = vec2(ray.x, -ray.y) / ray.z * 0.5 + 0.5;
            return textureSampleLevel(front, front_sampler, uv, 0.0);
        }
        let uv = vec2(-ray.x, -ray.y) / -ray.z * 0.5 + 0.5;
        return textureSampleLevel(back, back_sampler, uv, 0.0);
    }
    if axis.x >= axis.y {
        if ray.x >= 0.0 {
            let uv = vec2(-ray.z, -ray.y) / ray.x * 0.5 + 0.5;
            return textureSampleLevel(right, right_sampler, uv, 0.0);
        }
        let uv = vec2(ray.z, -ray.y) / -ray.x * 0.5 + 0.5;
        return textureSampleLevel(left, left_sampler, uv, 0.0);
    }
    if ray.y >= 0.0 {
        let uv = vec2(ray.x, ray.z) / ray.y * 0.5 + 0.5;
        return textureSampleLevel(up, up_sampler, uv, 0.0);
    }
    let uv = vec2(ray.x, -ray.z) / -ray.y * 0.5 + 0.5;
    return textureSampleLevel(down, down_sampler, uv, 0.0);
}
