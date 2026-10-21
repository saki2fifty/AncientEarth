// Original periodic material noise. No image data or terrain displacement.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};

struct GroundSettings {
    origin: vec4<f32>,
    east: vec4<f32>,
    up: vec4<f32>,
    south: vec4<f32>,
    sky_fill: vec4<f32>,
    eclipse: EclipseSettings,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> ground: GroundSettings;

fn hash_cell(cell: vec3<i32>, period: i32) -> f32 {
    let wrapped = ((cell % vec3(period)) + vec3(period)) % vec3(period);
    let c = vec3<u32>(wrapped);
    var h = c.x * 1664525u + c.y * 1013904223u + c.z * 374761393u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) / 4294967295.0;
}

// Value plus analytic gradient in ECEF metres. Scales divide the4096m period.
fn band(position: vec3<f32>, scale: f32, footprint: f32) -> vec4<f32> {
    let periodic_position = position - floor(position / 4096.0) * 4096.0;
    let p = periodic_position / scale;
    let cell = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let du = 6.0 * f * (1.0 - f) / scale;
    let period = i32(4096.0 / scale);
    let a = hash_cell(cell, period);
    let b = hash_cell(cell + vec3(1,0,0), period);
    let c = hash_cell(cell + vec3(0,1,0), period);
    let d = hash_cell(cell + vec3(1,1,0), period);
    let e = hash_cell(cell + vec3(0,0,1), period);
    let f1 = hash_cell(cell + vec3(1,0,1), period);
    let g = hash_cell(cell + vec3(0,1,1), period);
    let h = hash_cell(cell + vec3(1,1,1), period);
    let lo = mix(mix(a,b,u.x), mix(c,d,u.x), u.y);
    let hi = mix(mix(e,f1,u.x), mix(g,h,u.x), u.y);
    let value = mix(lo,hi,u.z);
    let gradient = vec3(
        mix(mix(b-a,d-c,u.y), mix(f1-e,h-g,u.y),u.z),
        mix(mix(c-a,d-b,u.x), mix(g-e,h-f1,u.x),u.z),
        hi-lo
    ) * du;
    // Suppression, not an exact bandlimit: derivatives are evaluated uniformly.
    let visible = 1.0 - smoothstep(0.06, 0.22, footprint / scale);
    return vec4(mix(0.5, value, visible), gradient * visible);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    var input = pbr_input_from_standard_material(in, front);
    let basis = mat3x3(ground.east.xyz, ground.up.xyz, ground.south.xyz);
    let ecef = ground.origin.xyz + basis * in.world_position.xyz;
    let footprint = max(length(dpdx(ecef)), length(dpdy(ecef)));
    // Decorative close material should not become distant speckled geography.
    let close = 1.0 - smoothstep(40.0,350.0,distance(view.world_position,in.world_position.xyz));
    let mean = vec4(0.5,0.0,0.0,0.0);
    let broad = mix(mean,band(ecef, 16.0, footprint),close);
    let clumps = mix(mean,band(ecef, 2.0, footprint),close);
    let grains = mix(mean,band(ecef, 0.125, footprint),close);
    let fine = mix(mean,band(ecef, 0.03125, footprint),close);
    let original = input.material.base_color.rgb;
    let vegetation = smoothstep(0.015, 0.09, original.g - original.r);
    let dry = broad.x * 0.55 + clumps.x * 0.45;
    let soil = vec3(0.135, 0.12, 0.055);
    let turf = mix(vec3(0.085,0.18,0.040), vec3(0.15,0.24,0.075), dry);
    let meadow = mix(turf, soil, 0.12 + 0.20 * clumps.x);
    let stone = original * (0.88 + 0.24 * broad.x);
    let grain_color = 0.90 + 0.16 * grains.x + 0.04 * fine.x;
    input.material.base_color = vec4(mix(stone, meadow, vegetation) * grain_color, 1.0);
    input.material.perceptual_roughness = clamp(0.78 + 0.16 * grains.x + 0.05 * clumps.x, 0.78, 0.99);

    // Analytic height gradient changes shading only; keep it tangent and bounded.
    let ecef_gradient = 0.012 * grains.yzw + 0.002 * fine.yzw + 0.015 * clumps.yzw;
    let local_gradient = transpose(basis) * ecef_gradient;
    let tangent = local_gradient - input.N * dot(local_gradient, input.N);
    let bounded = tangent / max(1.0, length(tangent) / 0.35);
    input.N = normalize(input.N - bounded);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(input);
    let solar_visibility = eclipse_solar_visibility(in.world_position.xyz,ground.eclipse);
    // This non-emissive scene has only sunlight and solar-derived ambient.
    // Existing terrain/grass shadow attenuation is already in the PBR result.
    out.color = vec4(out.color.rgb*solar_visibility,out.color.a);
    // Illustrative diffuse clear-sky radiance, not a calibrated atmosphere.
    // This term uses exposure once, like Bevy's other indirect illumination.
    let diffuse_fill = input.material.base_color.rgb * ground.sky_fill.rgb
        * (0.35 + 0.65 * max(input.N.y,0.0))
        * eclipse_sky_response(solar_visibility);
    out.color = vec4(out.color.rgb + diffuse_fill * view.exposure,out.color.a);
    out.color = main_pass_post_lighting_processing(input, out.color);
    return out;
}
