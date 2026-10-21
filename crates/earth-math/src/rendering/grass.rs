//! Original, illustrative grass. Every root follows the canonical surface.
//! GPU deformation and the sight query share bounded f32 wind uniforms and
//! quantized local vertices; GPU fused arithmetic/rasterization can still round.
use super::{LocalFrame, RenderMesh};
use crate::{
    EnuBasis, Geodetic, Observer, Ray, RayHit, TerrainScene, Vec3d, WGS84_A_M, WGS84_E2,
    ecef_to_geodetic, geodetic_to_ecef,
};
use std::collections::HashMap;

pub const GRASS_VERTEX_COUNT: usize = 9;
pub const GRASS_TRIANGLES: [[usize; 3]; 7] = [
    [0, 1, 2],
    [1, 3, 2],
    [2, 3, 4],
    [3, 5, 4],
    [4, 5, 6],
    [5, 7, 6],
    [6, 7, 8],
];
pub const GRASS_FADE_METRES: f32 = 3.0;
const CANDIDATE_GUARD_M: f64 = 5.0;

#[derive(Clone, Debug)]
pub struct GrassBlade {
    pub base: Vec3d,
    pub up: Vec3d,
    pub side: Vec3d,
    pub wind: Vec3d,
    /// Tangential rest displacement at the tip, in metres.
    pub lean: Vec3d,
    pub height_m: f64,
    pub width_m: f64,
    pub phase: f64,
    pub gust_phase: f64,
    pub cutoff_m: f64,
    pub color: [f32; 4],
}

/// Static mesh and deformation attributes. Roots contain local xyz and the
/// per-blade disappearance distance. Shadow passes use the same eye and weights.
#[derive(Default, Debug)]
pub struct GrassRenderMesh {
    pub mesh: RenderMesh,
    pub wind_a: Vec<[f32; 3]>,
    pub wind_b: Vec<[f32; 3]>,
    pub wind_c: Vec<[f32; 3]>,
    pub wind_d: Vec<[f32; 3]>,
    pub roots: Vec<[f32; 4]>,
}

pub fn grass_blades(scene: &TerrainScene, observer: Observer) -> Vec<GrassBlade> {
    grass_blades_with_quality(scene, observer, 1)
}

/// Deterministic jittered ten-centimetre cells. Quality changes the smooth
/// distance distribution, not terrain heights. Longitude indices wrap on each
/// row, including at the date line. Rows approaching a pole contain fewer cells.
pub fn grass_blades_with_quality(
    scene: &TerrainScene,
    observer: Observer,
    quality: usize,
) -> Vec<GrassBlade> {
    GrassPlacementCache::default().rebuild(scene, observer, quality)
}

/// Retains canonical ground queries between nearby placements. Call clear()
/// whenever the active immutable TerrainScene changes.
#[derive(Default, Debug)]
pub struct GrassPlacementCache {
    anchors: HashMap<[i64; 2], (Vec3d, Vec3d)>,
}

impl GrassPlacementCache {
    pub fn clear(&mut self) {
        self.anchors.clear();
    }

    pub fn rebuild(
        &mut self,
        scene: &TerrainScene,
        observer: Observer,
        quality: usize,
    ) -> Vec<GrassBlade> {
        grass_with_cached_anchors(scene, observer, quality, &mut self.anchors)
    }
}

fn grass_with_cached_anchors(
    scene: &TerrainScene,
    observer: Observer,
    quality: usize,
    anchors: &mut HashMap<[i64; 2], (Vec3d, Vec3d)>,
) -> Vec<GrassBlade> {
    let maximum_cutoff = [12.0, 18.0, 22.0, 26.0][quality.min(3)];
    let Ok(ground) = scene.ground(observer.feet) else {
        return Vec::new();
    };
    if observer.eye_geodetic().height_m - ground.hit.geodetic.height_m
        > maximum_cutoff + CANDIDATE_GUARD_M
    {
        return Vec::new();
    }
    let center = geodetic_to_ecef(observer.feet);
    let eye = observer.eye_position();
    let cell_m = 0.1;
    let row_count = (std::f64::consts::PI * WGS84_A_M / cell_m).round() as i64;
    let latitude_step = 180.0 / row_count as f64;
    let center_row = ((observer.feet.lat_deg + 90.0) / latitude_step).floor() as i64;
    // The minimum meridional radius bounds the local angular search rectangle.
    let minimum_radius = WGS84_A_M * (1.0 - WGS84_E2);
    let angular_radius = (maximum_cutoff + CANDIDATE_GUARD_M + cell_m) / minimum_radius;
    let row_span = (angular_radius.to_degrees() / latitude_step).ceil() as i64 + 2;
    let extreme_latitude = (observer.feet.lat_deg.abs().to_radians() + angular_radius)
        .min(std::f64::consts::FRAC_PI_2);
    let longitude_radius = if extreme_latitude >= std::f64::consts::FRAC_PI_2 {
        std::f64::consts::PI
    } else {
        (angular_radius / extreme_latitude.cos()).min(std::f64::consts::PI)
    };
    let mut blades = Vec::new();
    let mut retained = HashMap::with_capacity(anchors.len());
    for row in (center_row - row_span).max(0)..=(center_row + row_span).min(row_count - 1) {
        let row_latitude = (row as f64 + 0.5) * latitude_step - 90.0;
        let columns = (std::f64::consts::TAU * WGS84_A_M * row_latitude.to_radians().cos().abs()
            / cell_m)
            .round()
            .max(1.0) as i64;
        let longitude_step = 360.0 / columns as f64;
        let center_column = ((observer.feet.lon_deg + 180.0) / longitude_step).floor() as i64;
        let column_span = (longitude_radius.to_degrees() / longitude_step).ceil() as i64 + 2;
        let count = (2 * column_span + 1).min(columns);
        for offset in 0..count {
            let column = (center_column - column_span + offset).rem_euclid(columns);
            let key = row.wrapping_mul(73_856_093) ^ column.wrapping_mul(19_349_663);
            let latitude = (row as f64 + hash01(key)) * latitude_step - 90.0;
            let longitude = (column as f64 + hash01(key.wrapping_add(11))) * longitude_step - 180.0;
            let cutoff_m = 6.0 + (maximum_cutoff - 6.0) * hash01(key.wrapping_add(19));
            // Spend most geometry on low, spreading turf nearby. The sparse
            // taller subset is fixed in world coordinates at every quality.
            let primary_retained = hash01(key.wrapping_add(41)) < 0.25;
            let cell_cutoff = if primary_retained { cutoff_m } else { 6.0 };
            // Keep all root offsets in this shared margin before leaf culls.
            let candidate_radius = cell_cutoff.max(6.0) + CANDIDATE_GUARD_M + 2.0 * cell_m;
            let Ok(position) = Geodetic::new(latitude, longitude, 0.0) else {
                continue;
            };
            // Cheap reference-position cull precedes the canonical ground query.
            let reference = geodetic_to_ecef(Geodetic {
                height_m: observer.feet.height_m,
                ..position
            });
            if reference.distance(center) > candidate_radius {
                continue;
            }
            let primary_key = [row, 4 * column];
            let (base, up) = if let Some(&anchor) = anchors.get(&primary_key) {
                anchor
            } else {
                let Ok(hit) = scene.ground(position) else {
                    continue;
                };
                if !hit.grass_allowed {
                    continue;
                }
                (hit.hit.position, hit.hit.normal)
            };
            let basis = EnuBasis::at(position);
            if up.dot(basis.up) < 0.75 || base.distance(eye) > candidate_radius {
                continue;
            }
            retained.insert(primary_key, (base, up));
            let angle = hash01(key.wrapping_add(3)) * std::f64::consts::TAU;
            let raw_side = basis.east * angle.cos() + basis.north * angle.sin();
            let side = (raw_side - up * raw_side.dot(up)).normalized().unwrap();
            let raw_wind = basis.east * 0.8 + basis.north * 0.6;
            let wind = (raw_wind - up * raw_wind.dot(up)).normalized().unwrap();
            let height_m = 0.075 + 0.115 * hash01(key.wrapping_add(7));
            let lean = (up.cross(side) * (0.35 + 0.55 * hash01(key.wrapping_add(23)))
                + side * (0.30 * (hash01(key.wrapping_add(29)) - 0.5)))
                * height_m;
            let hue = hash01(key.wrapping_add(31)) as f32;
            let dry = if hash01(key.wrapping_add(37)) > 0.94 {
                0.6
            } else {
                0.0
            };
            if primary_retained && base.distance(eye) <= cutoff_m + CANDIDATE_GUARD_M {
                blades.push(GrassBlade {
                    base,
                    up,
                    side,
                    wind,
                    lean,
                    height_m,
                    width_m: 0.009 + 0.010 * hash01(key.wrapping_add(9)),
                    phase: hash01(key.wrapping_add(13)) * std::f64::consts::TAU,
                    gust_phase: (base.dot(Vec3d::new(0.63, 0.41, 0.66)) * 0.7)
                        .rem_euclid(std::f64::consts::TAU),
                    cutoff_m,
                    color: [
                        0.12 + 0.11 * hue + 0.13 * dry,
                        0.27 + 0.14 * hue,
                        0.025 + 0.035 * hue + 0.06 * dry,
                        1.0,
                    ],
                });
            }
            // Three independently grounded, spreading leaves fill the near
            // turf. Short cutoffs replace far shoots within a similar budget.
            for leaf in 0..3 {
                let leaf_key = key.wrapping_add(101 * (leaf + 1));
                let basal_cutoff = 4.0 + 2.0 * hash01(leaf_key.wrapping_add(43));
                if base.distance(eye) > basal_cutoff + CANDIDATE_GUARD_M + cell_m * 2.0 {
                    continue;
                }
                let basal_position = Geodetic::new(
                    (row as f64 + hash01(leaf_key.wrapping_add(47))) * latitude_step - 90.0,
                    (column as f64 + hash01(leaf_key.wrapping_add(53))) * longitude_step - 180.0,
                    0.0,
                )
                .expect("jittered cell remains in the geographic domain");
                let basal_key = [row, 4 * column + leaf + 1];
                let (basal_base, basal_up) = if let Some(&anchor) = anchors.get(&basal_key) {
                    anchor
                } else {
                    let Ok(hit) = scene.ground(basal_position) else {
                        continue;
                    };
                    if !hit.grass_allowed {
                        continue;
                    }
                    (hit.hit.position, hit.hit.normal)
                };
                let basal_basis = EnuBasis::at(basal_position);
                if basal_up.dot(basal_basis.up) < 0.75
                    || basal_base.distance(eye) > basal_cutoff + CANDIDATE_GUARD_M
                {
                    continue;
                }
                retained.insert(basal_key, (basal_base, basal_up));
                let basal_angle = std::f64::consts::TAU * hash01(leaf_key.wrapping_add(59));
                let raw_side =
                    basal_basis.east * basal_angle.cos() + basal_basis.north * basal_angle.sin();
                let basal_side = (raw_side - basal_up * raw_side.dot(basal_up))
                    .normalized()
                    .unwrap();
                let raw_wind = basal_basis.east * 0.8 + basal_basis.north * 0.6;
                let basal_wind = (raw_wind - basal_up * raw_wind.dot(basal_up))
                    .normalized()
                    .unwrap();
                let basal_height = 0.045 + 0.045 * hash01(leaf_key.wrapping_add(61));
                let basal_lean = (basal_up.cross(basal_side)
                    * (1.5 + hash01(leaf_key.wrapping_add(67)))
                    + basal_side * (0.3 * (hash01(leaf_key.wrapping_add(71)) - 0.5)))
                    * basal_height;
                blades.push(GrassBlade {
                    base: basal_base,
                    up: basal_up,
                    side: basal_side,
                    wind: basal_wind,
                    lean: basal_lean,
                    height_m: basal_height,
                    width_m: 0.014 + 0.010 * hash01(leaf_key.wrapping_add(73)),
                    phase: hash01(leaf_key.wrapping_add(79)) * std::f64::consts::TAU,
                    gust_phase: (basal_base.dot(Vec3d::new(0.63, 0.41, 0.66)) * 0.7)
                        .rem_euclid(std::f64::consts::TAU),
                    cutoff_m: basal_cutoff,
                    color: [
                        0.13 + 0.10 * hue,
                        0.31 + 0.13 * hue,
                        0.03 + 0.035 * hue,
                        1.0,
                    ],
                });
            }
        }
    }
    *anchors = retained;
    blades
}

fn hash01(value: i64) -> f64 {
    let mut bits = value as u64;
    bits ^= bits >> 30;
    bits = bits.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    bits ^= bits >> 27;
    bits = bits.wrapping_mul(0x94d0_49bb_1331_11eb);
    bits ^= bits >> 31;
    (bits >> 11) as f64 / ((1_u64 << 53) as f64)
}

/// Only bounded weights reach the GPU, avoiding a progressively coarser f32
/// timestamp. Nonfinite input uses the resting time-zero animation phase.
pub fn grass_wind(time_seconds: f64) -> [f32; 4] {
    let time = if time_seconds.is_finite() {
        time_seconds
    } else {
        0.0
    };
    let phase = |frequency: f64| time.rem_euclid(std::f64::consts::TAU / frequency) * frequency;
    let (a, b) = phase(1.35).sin_cos();
    let (c, d) = phase(3.8).sin_cos();
    [a as f32, b as f32, c as f32, d as f32]
}

/// Smooth, whole-blade shrink into its root, with zero endpoint derivative.
/// The shader uses this same cubic and the actual eye, also in shadow passes.
pub fn grass_fade(distance_m: f32, cutoff_m: f32) -> f32 {
    let x = ((cutoff_m - distance_m) / GRASS_FADE_METRES).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn wind_coefficients(blade: &GrassBlade) -> [Vec3d; 4] {
    let gust = blade.wind * (blade.height_m * 0.14);
    let flutter = (blade.wind * 0.65 + blade.side * 0.35) * (blade.height_m * 0.06);
    [
        gust * blade.gust_phase.cos(),
        gust * blade.gust_phase.sin(),
        flutter * blade.phase.cos(),
        flutter * blade.phase.sin(),
    ]
}

#[derive(Clone, Copy)]
struct BladeVertex {
    rest: Vec3d,
    coefficients: [Vec3d; 4],
    normal: Vec3d,
    color: [f32; 4],
}

fn blade_vertices(blade: &GrassBlade) -> [BladeVertex; GRASS_VERTEX_COUNT] {
    let coefficients = wind_coefficients(blade);
    std::array::from_fn(|index| {
        let stage = (index / 2) as f64 * 0.25;
        let curve = stage * stage;
        let twist = 0.3 * blade.phase.sin() * stage;
        let side = blade.side * twist.cos() + blade.up.cross(blade.side) * twist.sin();
        let half_width = if index == 8 {
            0.0
        } else {
            blade.width_m
                * 0.5
                * (1.0 - stage)
                * (1.0 + 0.5 * stage)
                * if index % 2 == 0 { -1.0 } else { 1.0 }
        };
        let rest = blade.base
            + blade.up * (blade.height_m * stage)
            + blade.lean * curve
            + side * half_width;
        let normal = side
            .cross(blade.up * blade.height_m + blade.lean * (2.0 * stage))
            .normalized()
            .unwrap_or(blade.up);
        let lightness = 0.55 + 0.80 * stage as f32;
        BladeVertex {
            rest,
            coefficients: coefficients.map(|value| value * curve),
            normal,
            color: [
                blade.color[0] * lightness,
                blade.color[1] * lightness,
                blade.color[2] * lightness,
                1.0,
            ],
        }
    })
}

pub fn grass_render_mesh(blades: &[GrassBlade], local: LocalFrame) -> GrassRenderMesh {
    let capacity = blades.len() * GRASS_VERTEX_COUNT;
    let mut output = GrassRenderMesh {
        mesh: RenderMesh {
            positions: Vec::with_capacity(capacity),
            normals: Vec::with_capacity(capacity),
            colors: Vec::with_capacity(capacity),
            indices: Vec::with_capacity(blades.len() * GRASS_TRIANGLES.len() * 3),
        },
        wind_a: Vec::with_capacity(capacity),
        wind_b: Vec::with_capacity(capacity),
        wind_c: Vec::with_capacity(capacity),
        wind_d: Vec::with_capacity(capacity),
        roots: Vec::with_capacity(capacity),
    };
    for blade in blades {
        let first = output.mesh.positions.len() as u32;
        let root = local.position(blade.base);
        for vertex in blade_vertices(blade) {
            output
                .mesh
                .vertex(local, vertex.rest, vertex.normal, vertex.color);
            let [a, b, c, d] = vertex.coefficients.map(|v| local.direction(v));
            output.wind_a.push(a);
            output.wind_b.push(b);
            output.wind_c.push(c);
            output.wind_d.push(d);
            output
                .roots
                .push([root[0], root[1], root[2], blade.cutoff_m as f32]);
        }
        for triangle in GRASS_TRIANGLES {
            output
                .mesh
                .indices
                .extend(triangle.map(|i| first + i as u32));
        }
    }
    output
}

/// CPU transcription of GPU vertex arithmetic for inspection and ray parity.
pub fn grass_deform_vertex(
    rest: [f32; 3],
    coefficients: [[f32; 3]; 4],
    root: [f32; 4],
    wind: [f32; 4],
    eye: [f32; 3],
) -> [f32; 3] {
    let delta = [eye[0] - root[0], eye[1] - root[1], eye[2] - root[2]];
    let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    let fade = grass_fade(distance, root[3]);
    std::array::from_fn(|axis| {
        let displaced = rest[axis]
            + coefficients[0][axis] * wind[0]
            + coefficients[1][axis] * wind[1]
            + coefficients[2][axis] * wind[2]
            + coefficients[3][axis] * wind[3];
        root[axis] + (displaced - root[axis]) * fade
    })
}

/// Retained CPU preview. The live renderer uploads the static mesh once and
/// deforms it with grass_wind; this helper evaluates the same local vertices.
pub fn grass_mesh(blades: &[GrassBlade], local: LocalFrame, time_seconds: f64) -> RenderMesh {
    let mut output = grass_render_mesh(blades, local);
    let wind = grass_wind(time_seconds);
    for (index, position) in output.mesh.positions.iter_mut().enumerate() {
        *position = grass_deform_vertex(
            *position,
            [
                output.wind_a[index],
                output.wind_b[index],
                output.wind_c[index],
                output.wind_d[index],
            ],
            output.roots[index],
            wind,
            [0.0; 3],
        );
    }
    output.mesh
}

fn bounding_radius(blade: &GrassBlade) -> f64 {
    blade.height_m.abs()
        + blade.lean.length()
        + blade.width_m.abs()
        + wind_coefficients(blade)
            .iter()
            .map(|v| v.length())
            .sum::<f64>()
}

fn sphere_may_hit(
    origin: Vec3d,
    direction: Vec3d,
    center: Vec3d,
    radius: f64,
    nearest: f64,
) -> bool {
    let relative = center - origin;
    let along = relative.dot(direction);
    let cross = relative.cross(direction);
    along + radius >= 0.0 && along - radius <= nearest && cross.dot(cross) <= radius * radius
}

/// Conservative world-space bound of every static local vertex under the
/// allowed wind weights and LOD fade. Rebuild with the mesh/floating origin.
#[derive(Clone, Copy, Debug)]
pub struct GrassRayBounds {
    pub center: Vec3d,
    pub radius_m: f64,
}

pub fn grass_ray_bounds(blades: &[GrassBlade], local: LocalFrame) -> Vec<GrassRayBounds> {
    blades
        .iter()
        .map(|blade| {
            let radius = bounding_radius(blade);
            let rounding_guard = 64.0
                * f64::from(f32::EPSILON)
                * (blade.base.distance(local.origin) + radius).max(1.0);
            GrassRayBounds {
                center: blade.base,
                radius_m: radius + rounding_guard,
            }
        })
        .collect()
}

/// Query the actual quantized static vertex attributes, current bounded wind
/// uniform and eye-dependent LOD. No stale 30-Hz CPU pose is involved. GPU fused
/// operations and raster interpolation can differ by small f32 rounding amounts.
pub fn grass_intersection_rendered(
    blades: &[GrassBlade],
    local: LocalFrame,
    wind: [f32; 4],
    ray: Ray,
) -> Option<RayHit> {
    grass_intersection_cached(blades, local, &grass_ray_bounds(blades, local), wind, ray)
}

/// Live-renderer broad phase. `bounds` must match `blades` and `local`, as
/// produced by grass_ray_bounds during the latest static mesh rebuild.
pub fn grass_intersection_cached(
    blades: &[GrassBlade],
    local: LocalFrame,
    bounds: &[GrassRayBounds],
    wind: [f32; 4],
    ray: Ray,
) -> Option<RayHit> {
    let direction = ray.direction.normalized()?;
    if blades.len() != bounds.len()
        || !ray.origin.is_finite()
        || wind
            .iter()
            .any(|value| !value.is_finite() || value.abs() > 1.0)
    {
        return None;
    }
    let eye = local.position(ray.origin);
    let mut nearest = None;
    let mut minimum = f64::INFINITY;
    for (blade, bound) in blades.iter().zip(bounds) {
        if !sphere_may_hit(ray.origin, direction, bound.center, bound.radius_m, minimum) {
            continue;
        }
        let root_position = local.position(blade.base);
        let root = [
            root_position[0],
            root_position[1],
            root_position[2],
            blade.cutoff_m as f32,
        ];
        let points = blade_vertices(blade).map(|vertex| {
            let xyz = grass_deform_vertex(
                local.position(vertex.rest),
                vertex.coefficients.map(|v| local.direction(v)),
                root,
                wind,
                eye,
            );
            local.origin + local.basis.east * f64::from(xyz[0]) + local.basis.up * f64::from(xyz[1])
                - local.basis.north * f64::from(xyz[2])
        });
        intersect_blade(points, ray.origin, direction, &mut nearest, &mut minimum);
    }
    nearest
}

/// Nominal f64 blade query without distance LOD, retained for isolated geometry
/// tests. The live GPU renderer uses grass_intersection_rendered instead.
pub fn grass_intersection(blades: &[GrassBlade], time_seconds: f64, ray: Ray) -> Option<RayHit> {
    let direction = ray.direction.normalized()?;
    let wind = grass_wind(time_seconds);
    let mut nearest = None;
    let mut minimum = f64::INFINITY;
    for blade in blades {
        if !sphere_may_hit(
            ray.origin,
            direction,
            blade.base,
            bounding_radius(blade),
            minimum,
        ) {
            continue;
        }
        let points = blade_vertices(blade).map(|vertex| {
            vertex
                .coefficients
                .into_iter()
                .zip(wind)
                .fold(vertex.rest, |point, (coefficient, weight)| {
                    point + coefficient * f64::from(weight)
                })
        });
        intersect_blade(points, ray.origin, direction, &mut nearest, &mut minimum);
    }
    nearest
}

fn intersect_blade(
    points: [Vec3d; GRASS_VERTEX_COUNT],
    origin: Vec3d,
    direction: Vec3d,
    nearest: &mut Option<RayHit>,
    minimum: &mut f64,
) {
    for [ia, ib, ic] in GRASS_TRIANGLES {
        let [a, b, c] = [points[ia], points[ib], points[ic]];
        let ab = b - a;
        let ac = c - a;
        let p = direction.cross(ac);
        let determinant = ab.dot(p);
        if determinant.abs() <= 64.0 * f64::EPSILON * ab.length() * ac.length() {
            continue;
        }
        let offset = origin - a;
        let u = offset.dot(p) / determinant;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = offset.cross(ab);
        let v = direction.dot(q) / determinant;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let distance = ac.dot(q) / determinant;
        if distance >= 0.0 && distance < *minimum {
            let position = origin + direction * distance;
            let Ok(geodetic) = ecef_to_geodetic(position) else {
                continue;
            };
            let Some(normal) = ab.cross(ac).normalized() else {
                continue;
            };
            *minimum = distance;
            *nearest = Some(RayHit {
                distance_m: distance,
                position,
                normal,
                geodetic,
            });
        }
    }
}
