//! Native-resolution curved-ray presentation, separate from straight-ray PBR.
#[cfg(test)]
mod review;
use super::{
    CameraRole, Explorer, FishQuad, SceneObject,
    materials::{EclipseSettings, GroundSettings, water_phase},
};
use ancient_earth::simulation::AstronomicalScene;
use bevy::{
    asset::uuid_handle,
    ecs::system::SystemParam,
    prelude::*,
    render::{
        render_resource::{AsBindGroup, ShaderType},
        storage::ShaderBuffer,
    },
    shader::ShaderRef,
    sprite_render::{Material2d, Material2dPlugin},
};
use earth_math::{
    TerrainScene, Vec3d,
    atmosphere::{AtmosphereProfile, gpu::GpuEllipsoid},
    rendering::LocalFrame,
};

const SHADER: Handle<Shader> = uuid_handle!("5b38ee2c-105a-474a-9a25-e7942efc32cf");
#[derive(Clone, Debug, ShaderType)]
pub struct Settings {
    q0: Vec4,
    q1: Vec4,
    q2: Vec4,
    origin: Vec4,
    center: Vec4,
    latitude: Vec4,
    right: Vec4,
    up: Vec4,
    forward: Vec4,
    lens: Vec4,
    budget: Vec4,
    sun: Vec4,
    moon: Vec4,
    moon_light: Vec4,
    environment: Vec4,
    icrf0: Vec4,
    icrf1: Vec4,
    icrf2: Vec4,
    ground_origin: Vec4,
    east: Vec4,
    vertical: Vec4,
    south: Vec4,
    phase: Vec4,
    region0: [Vec4; 4],
    region1: [Vec4; 4],
    layers: [Vec4; 128],
    eclipse: EclipseSettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            q0: Vec4::ZERO,
            q1: Vec4::ZERO,
            q2: Vec4::ZERO,
            origin: Vec4::ZERO,
            center: Vec4::ZERO,
            latitude: Vec4::ZERO,
            right: Vec4::ZERO,
            up: Vec4::ZERO,
            forward: Vec4::ZERO,
            lens: Vec4::ZERO,
            budget: Vec4::ZERO,
            sun: Vec4::ZERO,
            moon: Vec4::ZERO,
            moon_light: Vec4::ZERO,
            environment: Vec4::ZERO,
            icrf0: Vec4::ZERO,
            icrf1: Vec4::ZERO,
            icrf2: Vec4::ZERO,
            ground_origin: Vec4::ZERO,
            east: Vec4::ZERO,
            vertical: Vec4::ZERO,
            south: Vec4::ZERO,
            phase: Vec4::ZERO,
            region0: [Vec4::ZERO; 4],
            region1: [Vec4::ZERO; 4],
            layers: [Vec4::ZERO; 128],
            eclipse: EclipseSettings::default(),
        }
    }
}
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct RefractionMaterial {
    #[uniform(0)]
    settings: Settings,
    #[storage(1, read_only)]
    boundaries: Handle<ShaderBuffer>,
}
impl Material2d for RefractionMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}
#[derive(Component)]
pub struct RefractionQuad;
#[derive(Resource)]
pub struct RefractionDisplay {
    material: Handle<RefractionMaterial>,
    buffer: Handle<ShaderBuffer>,
    scene: usize,
    regions: Vec<Region>,
}
struct Region {
    bounds: [f64; 4],
    width: usize,
    height: usize,
    offset: usize,
}
impl Region {
    fn uniforms(&self, eye: earth_math::Geodetic) -> (Vec4, Vec4) {
        let [west, south, east, north] = self.bounds;
        let longitude = (eye.lon_deg - west + 180.0).rem_euclid(360.0) - 180.0;
        (
            Vec4::new(
                (north - eye.lat_deg).to_radians() as f32,
                longitude.to_radians() as f32,
                (3.0 * (self.height - 1) as f64 / (north - south).to_radians()) as f32,
                (3.0 * (self.width - 1) as f64 / (east - west).to_radians()) as f32,
            ),
            Vec4::new(
                (self.width - 1) as f32,
                (self.height - 1) as f32,
                self.offset as f32,
                0.0,
            ),
        )
    }
}

pub fn install(app: &mut App) {
    app.add_plugins(Material2dPlugin::<RefractionMaterial>::default())
        .add_systems(Startup, setup);
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(
            SHADER.id(),
            Shader::from_wesl(
                format!(
                    "{}\n{}",
                    include_str!("../shaders/refraction.wesl"),
                    include_str!("../shaders/eclipse.wesl")
                ),
                "ancient_earth/refraction.wesl",
            ),
        )
        .expect("original optical shader");
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<RefractionMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let buffer = buffers.add(ShaderBuffer::from(vec![[0.0_f32; 4]]));
    let material = materials.add(RefractionMaterial {
        settings: Settings::default(),
        boundaries: buffer.clone(),
    });
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(material.clone()),
        Transform::from_xyz(0.0, 0.0, 0.1).with_scale(Vec3::new(1920.0, 1080.0, 1.0)),
        Visibility::Hidden,
        RefractionQuad,
    ));
    commands.insert_resource(RefractionDisplay {
        material,
        buffer,
        scene: 0,
        regions: Vec::new(),
    });
}
#[derive(SystemParam)]
pub struct Parameters<'w, 's> {
    display: ResMut<'w, RefractionDisplay>,
    materials: ResMut<'w, Assets<RefractionMaterial>>,
    buffers: ResMut<'w, Assets<ShaderBuffer>>,
    #[allow(clippy::type_complexity)]
    quad: Query<
        'w,
        's,
        (&'static mut Transform, &'static mut Visibility),
        (
            With<RefractionQuad>,
            Without<CameraRole>,
            Without<SceneObject>,
            Without<DirectionalLight>,
            Without<FishQuad>,
        ),
    >,
}
impl Parameters<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        profile: Option<&AtmosphereProfile>,
        state: &Explorer,
        scene: &TerrainScene,
        astronomy: &AstronomicalScene,
        dimensions: (u32, u32),
        logical: Vec2,
        time: f64,
    ) {
        for (mut transform, mut visible) in &mut self.quad {
            *visible = if profile.is_some() {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            transform.scale = logical.extend(1.0);
        }
        let Some(profile) = profile else { return };
        let identity = scene as *const TerrainScene as usize;
        if identity != self.display.scene {
            let (data, regions) = boundary_data(scene);
            self.buffers
                .insert(self.display.buffer.id(), ShaderBuffer::from(data))
                .expect("retained boundary buffer");
            self.display.regions = regions;
            self.display.scene = identity;
        }
        let local = LocalFrame::at(state.observer);
        let geometry = GpuEllipsoid::at(state.observer);
        let frame = state.observer.frame();
        let lens = state.lens();
        let vector = |v: Vec3d| Vec3::from_array(local.direction(v)).extend(0.0);
        let ground = GroundSettings::at(local);
        let mut s = Settings {
            q0: Vec4::from_array(geometry.rows[0]),
            q1: Vec4::from_array(geometry.rows[1]),
            q2: Vec4::from_array(geometry.rows[2]),
            origin: Vec4::from_array(geometry.origin),
            center: Vec4::from_array(geometry.center),
            latitude: Vec4::from_array(geometry.latitude),
            right: vector(frame.right),
            up: vector(frame.up),
            forward: vector(frame.forward),
            lens: Vec4::new(
                dimensions.0 as f32 / dimensions.1 as f32,
                (lens.vertical_fov_deg.to_radians() * 0.5) as f32,
                u8::from(lens.projection == earth_math::Projection::EquidistantFisheye) as f32,
                if lens.projection == earth_math::Projection::EquidistantFisheye {
                    (f64::from(dimensions.1) / lens.vertical_fov_deg.to_radians()) as f32
                } else {
                    earth_math::rendering::focal_pixels(lens.vertical_fov_deg, dimensions.1) as f32
                },
            ),
            budget: Vec4::new(
                profile.layers().len() as f32,
                profile.top_height_m() as f32,
                (1.0 / (earth_math::WGS84_A_M * (1.0 - earth_math::WGS84_E2))
                    + profile.maximum_index_gradient()) as f32,
                u8::from(earth_math::atmosphere::gpu::display_supported(
                    state.observer,
                )) as f32,
            ),
            ground_origin: ground.origin,
            east: ground.east,
            vertical: ground.up,
            south: ground.south,
            phase: water_phase(time),
            ..default()
        };
        for (i, layer) in profile.layers().iter().enumerate() {
            let gradient = profile.layers().get(i + 1).map_or(0.0, |next| {
                (next.refractivity - layer.refractivity) / (next.height_m - layer.height_m)
            });
            s.layers[i] = Vec4::new(
                layer.height_m as f32,
                layer.refractivity as f32,
                gradient as f32,
                0.0,
            );
        }
        let eye = state.observer.eye_geodetic();
        for (i, region) in self.display.regions.iter().enumerate().take(4) {
            (s.region0[i], s.region1[i]) = region.uniforms(eye);
        }
        if let Some(astro) = astronomy.frame {
            let body = |p: Vec3d, r: f64| Vec3::from_array(local.position(p)).extend(r as f32);
            s.sun = body(astro.sun.apparent_center_ecef_m, astro.sun.radius_m);
            s.moon = body(astro.moon.apparent_center_ecef_m, astro.moon.radius_m);
            s.moon_light = vector(astro.moon_to_sun_direction_ecef);
            let sun_direction = (astro.sun.apparent_center_ecef_m - frame.origin)
                .normalized()
                .unwrap();
            let daylight =
                ((sun_direction.dot(local.basis.up) + 0.12) / 0.22).clamp(0.0, 1.0) as f32;
            let atmosphere = (1.0 - (eye.height_m / 120000.0).clamp(0.0, 1.0)) as f32;
            let eclipse = astro.solar_eclipse(frame.origin).ok();
            let visibility = eclipse.map_or(1.0, |e| e.visible_fraction) as f32;
            let response = 0.04 + 0.96 * visibility.sqrt();
            let stars = if eclipse.is_some_and(|e| e.kind == earth_math::astro::EclipseKind::Total)
            {
                0.96
            } else {
                0.0
            };
            s.environment = Vec4::new(
                daylight,
                atmosphere * response,
                1.0 - daylight * atmosphere * (1.0 - stars),
                0.0,
            );
            s.eclipse = EclipseSettings::from(astro.eclipse.gpu(local));
            let rotation = astro.icrf_to_ecef;
            s.icrf0 = vector(rotation.transform(Vec3d::new(1.0, 0.0, 0.0)));
            s.icrf1 = vector(rotation.transform(Vec3d::new(0.0, 1.0, 0.0)));
            s.icrf2 = vector(rotation.transform(Vec3d::new(0.0, 0.0, 1.0)));
        }
        if let Some(mut material) = self.materials.get_mut(&self.display.material) {
            material.settings = s;
        }
    }
}
fn boundary_data(scene: &TerrainScene) -> (Vec<[f32; 4]>, Vec<Region>) {
    let mut data = vec![[0.0; 4]];
    let mut regions = Vec::new();
    for patch in scene.patches().iter().take(4) {
        let Some(parameters) = patch.boundary_parameters() else {
            continue;
        };
        let Some(triangles) = patch.boundary_triangles() else {
            continue;
        };
        let (width, height, _) = patch.presentation_grid();
        let mut bins = vec![Vec::<usize>::new(); (width - 1) * (height - 1)];
        for (indices, code) in triangles {
            let points = indices.map(|index| parameters[index]);
            let row =
                ((points.iter().map(|p| p[0]).sum::<f64>() / 9.0).floor() as usize).min(height - 2);
            let col =
                ((points.iter().map(|p| p[1]).sum::<f64>() / 9.0).floor() as usize).min(width - 2);
            let id = data.len();
            bins[row * (width - 1) + col].push(id);
            data.push([
                points[0][0] as f32,
                points[0][1] as f32,
                points[1][0] as f32,
                points[1][1] as f32,
            ]);
            data.push([
                points[2][0] as f32,
                points[2][1] as f32,
                u8::from((1..=3).contains(&code)) as f32,
                0.0,
            ]);
        }
        let offset = data.len();
        data.resize(offset + bins.len(), [0.0; 4]);
        for (i, bin) in bins.into_iter().enumerate() {
            data[offset + i] = [data.len() as f32, bin.len() as f32, 0.0, 0.0];
            data.extend(bin.into_iter().map(|index| [index as f32, 0.0, 0.0, 0.0]));
        }
        regions.push(Region {
            bounds: patch.bounds(),
            width,
            height,
            offset,
        });
    }
    assert!(
        data.len() < 16_777_216,
        "boundary buffer indices must be exact f32 integers"
    );
    (data, regions)
}
