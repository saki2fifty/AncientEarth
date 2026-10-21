// Original shared wind deformation for color and shadow/depth passes.
import bevy_pbr::render::{
    mesh_functions,
    view_transformations::position_world_to_clip,
};
@if(PREPASS_PIPELINE)
import bevy_pbr::prepass::io::VertexOutput;
@else
import bevy_pbr::render::forward_io::VertexOutput;
@if(!PREPASS_PIPELINE)
import bevy_pbr::render::{
    forward_io::FragmentOutput,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};

struct GrassSettings {
    wind: vec4<f32>, eye: vec4<f32>, sky_fill: vec4<f32>, eclipse: EclipseSettings,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> grass: GrassSettings;
struct GrassVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(5) color: vec4<f32>,
    @location(8) wind_a: vec3<f32>,
    @location(9) wind_b: vec3<f32>,
    @location(10) wind_c: vec3<f32>,
    @location(11) wind_d: vec3<f32>,
    @location(12) root: vec4<f32>,
};
@vertex
fn vertex(in: GrassVertex) -> VertexOutput {
    let displaced = in.position
        + in.wind_a * grass.wind.x + in.wind_b * grass.wind.y
        + in.wind_c * grass.wind.z + in.wind_d * grass.wind.w;
    // Use actual observer eye for all views, including Sun shadow cameras.
    let t = clamp((in.root.w - distance(grass.eye.xyz,in.root.xyz))/3.0,0.0,1.0);
    let fade = t * t * (3.0 - 2.0 * t);
    let position = in.root.xyz + (displaced - in.root.xyz) * fade;
    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    var out: VertexOutput;
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local,vec4(position,1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
@if(!PREPASS_PIPELINE || NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(in.normal,in.instance_index);
@if(VERTEX_COLORS)
    out.color = in.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = in.instance_index;
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z,1.0);
}
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(in.instance_index,world_from_local[3]);
    return out;
}

@if(!PREPASS_PIPELINE)
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    let input = pbr_input_from_standard_material(in,front);
    // Illustrative diffuse skylight prevents thin back-facing leaves going black.
    // It is explicitly daylight-gated by the same astronomical Sun direction.
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(input);
    let solar_visibility = eclipse_solar_visibility(in.world_position.xyz,grass.eclipse);
    // Includes reflected/transmitted sunlight and solar-derived ambient only;
    // no emissive or independently powered light is present in this material.
    out.color = vec4(out.color.rgb*solar_visibility,out.color.a);
    // PBR's output is already exposed. Add exposed radiance once, before fog and
    // tone mapping. lightmap_light is ignored without Bevy's LIGHTMAP variant.
    let diffuse_fill = input.material.base_color.rgb * grass.sky_fill.rgb
        * eclipse_sky_response(solar_visibility);
    out.color = vec4(out.color.rgb + diffuse_fill * view.exposure,out.color.a);
    out.color = main_pass_post_lighting_processing(input,out.color);
    return out;
}
