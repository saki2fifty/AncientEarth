// Original illustrative lunar albedo and Lambert phase; no photograph.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
};
struct MoonSettings { illumination: vec4<f32> };
@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> moon: MoonSettings;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    let input = pbr_input_from_standard_material(in, front);
    let lit = max(dot(normalize(in.world_normal), moon.illumination.xyz),0.0);
    var out: FragmentOutput;
    out.color = vec4(input.material.base_color.rgb * lit + vec3(0.006),1.0);
    out.color = main_pass_post_lighting_processing(input,out.color);
    return out;
}
