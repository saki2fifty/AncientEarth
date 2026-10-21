// Original, illustrative water appearance. Scientific water vertices do not move.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};

struct GroundSettings {
    origin: vec4<f32>, east: vec4<f32>, up: vec4<f32>, south: vec4<f32>,
    sky_fill: vec4<f32>, eclipse: EclipseSettings,
};
struct WaterSettings {
    coordinates: GroundSettings,
    phase: vec4<f32>,
    environment: vec4<f32>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> water: WaterSettings;

// Integer spatial frequencies and wavelengths dividing4096 preserve the
// periodic ECEF anchor across origin rebases. These are shading slopes only.
fn ripple(position: vec3<f32>, footprint: f32, direction: vec3<f32>,
          wavelength: f32, phase: vec2<f32>, amplitude: f32) -> vec3<f32> {
    let spatial_phase = dot(position,direction) * (6.28318530718/wavelength);
    let pixel_phase = footprint * length(direction) * (6.28318530718/wavelength);
    let filtered = 1.0-smoothstep(0.2,0.8,pixel_phase);
    let cosine = cos(spatial_phase)*phase.y-sin(spatial_phase)*phase.x;
    return normalize(direction)*amplitude*cosine*filtered;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    var input = pbr_input_from_standard_material(in,front);
    let basis = mat3x3(water.coordinates.east.xyz,water.coordinates.up.xyz,water.coordinates.south.xyz);
    let ecef = water.coordinates.origin.xyz+basis*in.world_position.xyz;
    // Derivatives precede periodic wrapping and every branch.
    let footprint = max(length(dpdx(ecef)),length(dpdy(ecef)));
    let periodic = ecef-floor(ecef/4096.0)*4096.0;
    let phase_sum = vec2(water.phase.x*water.phase.w+water.phase.y*water.phase.z,
        water.phase.y*water.phase.w-water.phase.x*water.phase.z);
    let slope_ecef = ripple(periodic,footprint,vec3(1.0,2.0,0.0),64.0,water.phase.xy,0.035)
        + ripple(periodic,footprint,vec3(-2.0,1.0,1.0),16.0,water.phase.zw,0.018)
        + ripple(periodic,footprint,vec3(1.0,-1.0,3.0),4.0,phase_sum,0.008);
    let slope_local = transpose(basis)*slope_ecef;
    let tangent = slope_local-input.N*dot(slope_local,input.N);
    let bounded = tangent/max(1.0,length(tangent)/0.07);
    input.N = normalize(input.N-bounded);
    input.material.perceptual_roughness = 0.22;
    var out: FragmentOutput;
    let solar_visibility = eclipse_solar_visibility(in.world_position.xyz,water.coordinates.eclipse);
    out.color = apply_pbr_lighting(input);
    out.color = vec4(out.color.rgb*solar_visibility,out.color.a);

    // The sky mesh's illustrative colors, reflected about the shading normal.
    // Smooth its horizon brightness change: a hard hemispheric step would trace
    // sharp ripple contours in grazing telescope views. This is an artistic
    // environment approximation, not a reflection of shore objects or stars.
    let reflected = reflect(-input.V,input.N);
    let zenith = pow(max(reflected.y,0.0),0.55);
    let below = 0.8+0.2*smoothstep(-0.08,0.08,reflected.y);
    let sky_response = eclipse_sky_response(solar_visibility);
    let strength = water.environment.x*water.environment.y*below*sky_response;
    let sky_color = vec3(0.0003,0.0007,0.0018)+strength*
        mix(vec3(0.46,0.67,0.88),vec3(0.055,0.20,0.48),zenith);
    let f0 = 0.16*input.material.reflectance*input.material.reflectance;
    let grazing = 1.0-clamp(dot(input.N,input.V),0.0,1.0);
    let fresnel = f0+(vec3(1.0)-f0)*pow(grazing,5.0);
    let fill = input.material.base_color.rgb*water.coordinates.sky_fill.rgb*sky_response;
    // PBR and diffuse fill use exposure once. The sky is an unlit scene-linear
    // display approximation, so its reflected color is already in that space.
    out.color = vec4(out.color.rgb+fill*view.exposure+sky_color*fresnel,out.color.a);
    out.color = main_pass_post_lighting_processing(input,out.color);
    return out;
}
