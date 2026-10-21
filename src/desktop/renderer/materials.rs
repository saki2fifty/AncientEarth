//! Original procedural presentation materials. They never displace the terrain.
use bevy::{
    asset::uuid_handle,
    camera::primitives::Aabb,
    mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef},
    pbr::{
        ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
        MaterialPlugin,
    },
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
        VertexFormat,
    },
    shader::ShaderRef,
};
use earth_math::rendering::LocalFrame;

/// Sun-aligned, anchor-relative scientific geometry, packed identically to WESL.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct EclipseSettings {
    pub row_x: Vec4,
    pub row_y: Vec4,
    pub row_z: Vec4,
    pub sun: Vec4,
    pub moon: Vec4,
    pub sun_velocity: Vec4,
    pub moon_velocity: Vec4,
    pub observer_beta: Vec4,
    pub spin_over_c: Vec4,
}
impl From<earth_math::astro::GpuSolarEclipse> for EclipseSettings {
    fn from(value: earth_math::astro::GpuSolarEclipse) -> Self {
        Self {
            row_x: Vec4::from_array(value.local_to_shadow[0]),
            row_y: Vec4::from_array(value.local_to_shadow[1]),
            row_z: Vec4::from_array(value.local_to_shadow[2]),
            sun: Vec4::from_array(value.sun),
            moon: Vec4::from_array(value.moon),
            sun_velocity: Vec4::from_array(value.sun_velocity),
            moon_velocity: Vec4::from_array(value.moon_velocity),
            observer_beta: Vec4::from_array(value.observer_beta),
            spin_over_c: Vec4::from_array(value.spin_over_c),
        }
    }
}

const GROUND_SHADER: Handle<Shader> = uuid_handle!("8ec76db7-b528-499e-91a7-fad778eb3e7d");
const MOON_SHADER: Handle<Shader> = uuid_handle!("976c3c68-71e6-4921-aea0-028aeb83e3c2");
const GRASS_SHADER: Handle<Shader> = uuid_handle!("9e711805-c9c8-48d6-97b9-7ed9567ec4cb");
const WATER_SHADER: Handle<Shader> = uuid_handle!("a9330ea6-37b1-460b-b925-1fb9eecbdcaf");
const WIND_A: MeshVertexAttribute =
    MeshVertexAttribute::new("GrassWindA", 150000001, VertexFormat::Float32x3);
const WIND_B: MeshVertexAttribute =
    MeshVertexAttribute::new("GrassWindB", 150000002, VertexFormat::Float32x3);
const WIND_C: MeshVertexAttribute =
    MeshVertexAttribute::new("GrassWindC", 150000003, VertexFormat::Float32x3);
const WIND_D: MeshVertexAttribute =
    MeshVertexAttribute::new("GrassWindD", 150000004, VertexFormat::Float32x3);
const ROOT: MeshVertexAttribute =
    MeshVertexAttribute::new("GrassRoot", 150000005, VertexFormat::Float32x4);

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct GroundSettings {
    pub origin: Vec4,
    pub east: Vec4,
    pub up: Vec4,
    pub south: Vec4,
    pub sky_fill: Vec4,
    pub eclipse: EclipseSettings,
}
impl GroundSettings {
    pub fn at(local: LocalFrame) -> Self {
        let vector = |v: earth_math::Vec3d| Vec4::new(v.x as f32, v.y as f32, v.z as f32, 0.0);
        Self {
            origin: Vec4::new(
                local.origin.x.rem_euclid(4096.0) as f32,
                local.origin.y.rem_euclid(4096.0) as f32,
                local.origin.z.rem_euclid(4096.0) as f32,
                0.0,
            ),
            east: vector(local.basis.east),
            up: vector(local.basis.up),
            south: vector(-local.basis.north),
            sky_fill: Vec4::ZERO,
            eclipse: EclipseSettings::default(),
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GroundExtension {
    #[uniform(100)]
    pub settings: GroundSettings,
}
impl MaterialExtension for GroundExtension {
    fn fragment_shader() -> ShaderRef {
        GROUND_SHADER.into()
    }
}
pub type GroundMaterial = ExtendedMaterial<StandardMaterial, GroundExtension>;

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct WaterSettings {
    pub coordinates: GroundSettings,
    /// Bounded sin/cos pairs, avoiding progressively coarse f32 elapsed time.
    pub phase: Vec4,
    /// Daylight and atmosphere factors before local eclipse attenuation.
    pub environment: Vec4,
}
impl WaterSettings {
    pub fn at(local: LocalFrame) -> Self {
        Self {
            coordinates: GroundSettings::at(local),
            phase: water_phase(0.0),
            environment: Vec4::ZERO,
        }
    }
}
pub fn water_phase(elapsed_seconds: f64) -> Vec4 {
    let time = if elapsed_seconds.is_finite() {
        elapsed_seconds
    } else {
        0.0
    };
    let phase =
        |frequency: f64| (time.rem_euclid(std::f64::consts::TAU / frequency) * frequency).sin_cos();
    let (a, b) = phase(0.33);
    let (c, d) = phase(0.61);
    Vec4::new(a as f32, b as f32, c as f32, d as f32)
}
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WaterExtension {
    #[uniform(100)]
    pub settings: WaterSettings,
}
impl MaterialExtension for WaterExtension {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER.into()
    }
}
pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct MoonExtension {
    #[uniform(100)]
    pub illumination: Vec4,
}
impl MaterialExtension for MoonExtension {
    fn fragment_shader() -> ShaderRef {
        MOON_SHADER.into()
    }
}
pub type MoonMaterial = ExtendedMaterial<StandardMaterial, MoonExtension>;

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct GrassSettings {
    pub wind: Vec4,
    pub eye: Vec4,
    pub sky_fill: Vec4,
    pub eclipse: EclipseSettings,
}
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GrassExtension {
    #[uniform(100)]
    pub settings: GrassSettings,
}
impl MaterialExtension for GrassExtension {
    fn vertex_shader() -> ShaderRef {
        GRASS_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        GRASS_SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        GRASS_SHADER.into()
    }
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(5),
            WIND_A.at_shader_location(8),
            WIND_B.at_shader_location(9),
            WIND_C.at_shader_location(10),
            WIND_D.at_shader_location(11),
            ROOT.at_shader_location(12),
        ])?];
        Ok(())
    }
}
pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassExtension>;

pub fn grass_mesh(data: earth_math::rendering::GrassRenderMesh) -> (Mesh, Aabb) {
    if data.mesh.indices.is_empty() {
        let mut mesh = super::blank_mesh();
        for attribute in [WIND_A, WIND_B, WIND_C, WIND_D] {
            mesh.insert_attribute(attribute, vec![[0.0_f32; 3]; 3]);
        }
        mesh.insert_attribute(ROOT, vec![[0.0_f32; 4]; 3]);
        return (
            mesh,
            Aabb::from_min_max(Vec3::splat(-0.01), Vec3::splat(0.01)),
        );
    }
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for i in 0..data.mesh.positions.len() {
        let position = Vec3::from_array(data.mesh.positions[i]);
        let root = Vec3::from_array([data.roots[i][0], data.roots[i][1], data.roots[i][2]]);
        let envelope = Vec3::from_array(data.wind_a[i]).abs()
            + Vec3::from_array(data.wind_b[i]).abs()
            + Vec3::from_array(data.wind_c[i]).abs()
            + Vec3::from_array(data.wind_d[i]).abs();
        // Wind weights are in[-1,1]; the fade is a convex mix with the root.
        // Outward roundoff padding also prevents a degenerate culling volume.
        let padding = (position.abs() + root.abs() + envelope).max(Vec3::ONE)
            * (64.0 * f32::EPSILON)
            + Vec3::splat(0.001);
        low = low.min((position - envelope).min(root) - padding);
        high = high.max((position + envelope).max(root) + padding);
    }
    let mut mesh = super::mesh_from(data.mesh);
    mesh.insert_attribute(WIND_A, data.wind_a);
    mesh.insert_attribute(WIND_B, data.wind_b);
    mesh.insert_attribute(WIND_C, data.wind_c);
    mesh.insert_attribute(WIND_D, data.wind_d);
    mesh.insert_attribute(ROOT, data.roots);
    (mesh, Aabb::from_min_max(low, high))
}

pub fn install(app: &mut App) {
    app.add_plugins((
        MaterialPlugin::<GroundMaterial>::default(),
        MaterialPlugin::<MoonMaterial>::default(),
        MaterialPlugin::<GrassMaterial>::default(),
        MaterialPlugin::<WaterMaterial>::default(),
    ));
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(
            GROUND_SHADER.id(),
            Shader::from_wesl(
                format!(
                    "{}\n{}",
                    include_str!("../shaders/ground.wesl"),
                    include_str!("../shaders/eclipse.wesl")
                ),
                "ancient_earth/ground.wesl",
            ),
        )
        .expect("insert original ground shader");
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(
            MOON_SHADER.id(),
            Shader::from_wesl(
                include_str!("../shaders/moon.wesl"),
                "ancient_earth/moon.wesl",
            ),
        )
        .expect("insert original lunar shader");
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(
            GRASS_SHADER.id(),
            Shader::from_wesl(
                format!(
                    "{}\n{}",
                    include_str!("../shaders/grass.wesl"),
                    include_str!("../shaders/eclipse.wesl")
                ),
                "ancient_earth/grass.wesl",
            ),
        )
        .expect("insert original grass shader");
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(
            WATER_SHADER.id(),
            Shader::from_wesl(
                format!(
                    "{}\n{}",
                    include_str!("../shaders/water.wesl"),
                    include_str!("../shaders/eclipse.wesl")
                ),
                "ancient_earth/water.wesl",
            ),
        )
        .expect("insert original water shader");
}

#[cfg(test)]
mod review;
