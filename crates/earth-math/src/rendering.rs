//! Finite GPU presentation geometry. Boundary-only scenes measure the analytic
//! WGS84 ellipsoid; legacy terrain scenes retain their f64 triangle surface.
//!
//! Local axes are east, geodetic up, and south. Subtract the floating origin in
//! f64 before converting to GPU f32. No photographic or inferred geographic data.
use crate::{
    CameraFrame, EnuBasis, Geodetic, Observer, TerrainPatch, TerrainScene, Vec3d, WGS84_A_M,
    WGS84_B_M, WGS84_E2, ecef_to_geodetic, geodetic_to_ecef,
};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug)]
pub struct LocalFrame {
    pub origin: Vec3d,
    pub basis: EnuBasis,
}
impl LocalFrame {
    pub fn at(observer: Observer) -> Self {
        Self {
            origin: observer.frame().origin,
            basis: EnuBasis::at(observer.feet),
        }
    }
    pub fn direction(self, vector: Vec3d) -> [f32; 3] {
        [
            vector.dot(self.basis.east) as f32,
            vector.dot(self.basis.up) as f32,
            -vector.dot(self.basis.north) as f32,
        ]
    }
    pub fn position(self, position: Vec3d) -> [f32; 3] {
        self.direction(position - self.origin)
    }
    pub fn camera(self, frame: CameraFrame) -> ([f32; 3], [f32; 3], [f32; 3]) {
        (
            self.position(frame.origin),
            self.direction(frame.forward),
            self.direction(frame.up),
        )
    }
}

#[derive(Default, Debug)]
pub struct RenderMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}
impl RenderMesh {
    fn vertex(&mut self, local: LocalFrame, point: Vec3d, normal: Vec3d, color: [f32; 4]) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push(local.position(point));
        self.normals.push(local.direction(normal));
        self.colors.push(color);
        index
    }
}

/// Vertical perspective focal length in physical pixels. This is a display
/// metric, not a terrain accuracy statement.
pub fn focal_pixels(fov_degrees: f64, height: u32) -> f64 {
    height.max(1) as f64 / (2.0 * (0.5 * fov_degrees.to_radians()).tan())
}

/// Conservative whole-patch *geometric* LOD bound, with a focal-length/radial
/// distance screen heuristic. This is not a strict one-pixel projection bound
/// off-axis, especially at extreme fields of view. Every source post is retained nearby.
pub fn patch_stride(
    vertices: &[Vec3d],
    width: usize,
    height: usize,
    eye: Vec3d,
    focal: f64,
) -> usize {
    let center = (vertices[0] + vertices[vertices.len() - 1]) / 2.0;
    let radius = vertices
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0_f64, f64::max);
    let minimum_distance = (eye.distance(center) - radius).max(0.01);
    let mut selected = 1;
    for stride in [2_usize, 4, 8, 16, 32] {
        let mut diameter = 0.0_f64;
        for row in (0..height - 1).step_by(stride) {
            for col in (0..width - 1).step_by(stride) {
                let last_row = (row + stride).min(height - 1);
                let last_col = (col + stride).min(width - 1);
                let corner = vertices[row * width + col];
                // Every fine vertex is examined: extreme terrain cannot be
                // discarded merely because the block's corners are flat.
                let radius = (row..=last_row)
                    .flat_map(|y| (col..=last_col).map(move |x| vertices[y * width + x]))
                    .map(|p| p.distance(corner))
                    .fold(0.0_f64, f64::max);
                diameter = diameter.max(2.0 * radius);
            }
        }
        if diameter * focal / minimum_distance <= 1.0 {
            selected = stride;
        } else {
            break;
        }
    }
    selected
}

pub fn terrain_mesh(patch: &TerrainPatch, local: LocalFrame, focal: f64) -> (RenderMesh, usize) {
    if patch.requires_canonical_presentation() {
        // This generic entry point must also preserve a refined shoreline. The
        // desktop uses the separate materials below, but other callers must not
        // accidentally reconstruct only the legacy regular grid.
        let (mut land, water, stride) = terrain_water_mesh(patch, local, focal);
        let offset = land.positions.len() as u32;
        land.positions.extend(water.positions);
        land.normals.extend(water.normals);
        land.colors.extend(water.colors);
        land.indices
            .extend(water.indices.into_iter().map(|index| index + offset));
        return (land, stride);
    }
    let (width, height, vertices) = patch.presentation_grid();
    let stride = patch_stride(vertices, width, height, local.origin, focal);
    let mut rows: Vec<usize> = (0..height - 1).step_by(stride).collect();
    let mut columns: Vec<usize> = (0..width - 1).step_by(stride).collect();
    rows.push(height - 1);
    columns.push(width - 1);
    let mut mesh = RenderMesh::default();
    let points: Vec<Vec3d> = rows
        .iter()
        .flat_map(|row| {
            columns
                .iter()
                .map(move |column| vertices[row * width + column])
        })
        .collect();
    let mut normals = vec![Vec3d::ZERO; points.len()];
    for row in 0..rows.len() - 1 {
        for column in 0..columns.len() - 1 {
            let nw = row * columns.len() + column;
            let ne = nw + 1;
            let sw = nw + columns.len();
            let se = sw + 1;
            for indices in [[nw, sw, se], [nw, se, ne]] {
                let [a, b, c] = indices.map(|i| points[i]);
                let normal = (b - a).cross(c - a);
                for i in indices {
                    normals[i] = normals[i] + normal;
                }
                mesh.indices.extend(indices.map(|i| i as u32));
            }
        }
    }
    for (index, position) in points.into_iter().enumerate() {
        let geodetic = ecef_to_geodetic(position).expect("canonical source post");
        let normal = if patch.is_boundary_only() {
            EnuBasis::at(geodetic).up
        } else {
            normals[index]
                .normalized()
                .unwrap_or(EnuBasis::at(geodetic).up)
        };
        let color = if patch.is_boundary_only() {
            ELLIPSOID_LAND_COLOR
        } else {
            terrain_color(geodetic, normal)
        };
        mesh.vertex(local, position, normal, color);
    }
    (mesh, stride)
}

fn terrain_color(geodetic: Geodetic, normal: Vec3d) -> [f32; 4] {
    // An explicitly illustrative altitude/slope material, not land cover.
    let rock = ((geodetic.height_m - 1_700.0) / 1_500.0).clamp(0.0, 1.0) as f32;
    let slope = (1.0 - normal.dot(EnuBasis::at(geodetic).up)).clamp(0.0, 1.0) as f32;
    let blend = (rock + slope).min(1.0);
    [
        0.18 + 0.15 * blend,
        0.27 + 0.03 * blend,
        0.11 + 0.16 * blend,
        1.0,
    ]
}

// Original illustrative land appearance; no height, slope or source coverage
// is encoded in this color. The global fallback uses the identical material.
const ELLIPSOID_LAND_COLOR: [f32; 4] = [0.18, 0.27, 0.11, 1.0];

/// Disjoint land/water material meshes from the same canonical triangles.
/// Water-bearing and refined patches retain every canonical triangle so a coarser
/// mesh cannot bridge a classified shoreline or create overlapping surfaces.
/// Other patches retain the existing geometric LOD policy.
pub fn terrain_water_mesh(
    patch: &TerrainPatch,
    local: LocalFrame,
    focal: f64,
) -> (RenderMesh, RenderMesh, usize) {
    if !patch.requires_canonical_presentation() {
        let (land, stride) = terrain_mesh(patch, local, focal);
        return (land, RenderMesh::default(), stride);
    }
    // Refined shoreline vertices need not lie on the legacy regular grid. The
    // shared canonical vertex pool supplies both material meshes exactly.
    let vertices = patch.canonical_vertices();
    let key = |v: Vec3d| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()];
    let lookup: HashMap<[u64; 3], usize> = vertices
        .iter()
        .enumerate()
        .map(|(index, &point)| (key(point), index))
        .collect();
    let mut normals = [
        vec![Vec3d::ZERO; vertices.len()],
        vec![Vec3d::ZERO; vertices.len()],
    ];
    let mut indices = [Vec::new(), Vec::new()];
    for (triangle, water) in patch.presentation_triangles() {
        let material = usize::from(water.is_some());
        let ids = triangle.map(|p| lookup[&key(p)]);
        let [a, b, c] = triangle;
        let normal = (b - a).cross(c - a);
        for i in ids {
            normals[material][i] = normals[material][i] + normal;
        }
        indices[material].extend(ids.map(|i| i as u32));
    }
    let mut meshes = [RenderMesh::default(), RenderMesh::default()];
    for material in 0..2 {
        let mut remap = vec![u32::MAX; vertices.len()];
        for &index in &indices[material] {
            let index = index as usize;
            if remap[index] != u32::MAX {
                continue;
            }
            let point = vertices[index];
            let geodetic = patch
                .boundary_geodetic(index)
                .unwrap_or_else(|| ecef_to_geodetic(point).expect("canonical source post"));
            // A boundary changes the material, never the smooth ellipsoid
            // normal. Both material copies receive exactly the same normal.
            let normal = if patch.is_boundary_only() {
                EnuBasis::at(geodetic).up
            } else {
                normals[material][index]
                    .normalized()
                    .unwrap_or(EnuBasis::at(geodetic).up)
            };
            let color = if material == 1 {
                [1.0; 4]
            } else if patch.is_boundary_only() {
                ELLIPSOID_LAND_COLOR
            } else {
                terrain_color(geodetic, normal)
            };
            remap[index] = meshes[material].vertex(local, point, normal, color);
        }
        meshes[material].indices = indices[material]
            .iter()
            .map(|i| remap[*i as usize])
            .collect();
    }
    let [land, water] = meshes;
    (land, water, 1)
}

/// Independent reference-ellipsoid mesh, excluding regional rectangles. Each
/// adaptive cell shares neighboring edge posts. For the active boundary-only
/// application, use [`boundary_scene_meshes`] instead: it also synchronizes
/// reference and regional perimeter subdivisions. Legacy terrain openings only
/// approximate their Cartesian triangle footprints.
pub fn reference_mesh(scene: &TerrainScene, local: LocalFrame, focal: f64) -> RenderMesh {
    reference_mesh_impl(scene, local, focal, None)
}

fn reference_mesh_impl(
    scene: &TerrainScene,
    local: LocalFrame,
    focal: f64,
    mut regional_edges: Option<&mut [Vec<Geodetic>]>,
) -> RenderMesh {
    let mut latitudes: Vec<f64> = (-9..=9).map(|i| i as f64 * 10.0).collect();
    let mut longitudes: Vec<f64> = (-18..=18).map(|i| i as f64 * 10.0).collect();
    for patch in scene.patches() {
        let [west, south, east, north] = patch.bounds();
        latitudes.extend([south, north]);
        longitudes.extend([west, east]);
    }
    latitudes.sort_by(f64::total_cmp);
    longitudes.sort_by(f64::total_cmp);
    latitudes.dedup();
    longitudes.dedup();
    let mut cells = Vec::new();
    for lat in latitudes.windows(2) {
        for lon in longitudes.windows(2) {
            let midpoint =
                Geodetic::new((lat[0] + lat[1]) * 0.5, (lon[0] + lon[1]) * 0.5, 0.0).unwrap();
            if scene.patches().iter().any(|p| p.contains(midpoint)) {
                continue;
            }
            refine_cell(
                [lon[0], lat[0], lon[1], lat[1]],
                local.origin,
                focal,
                0,
                &mut cells,
            );
        }
    }
    let mut horizontal: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
    let mut vertical: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
    for &[west, south, east, north] in &cells {
        for (lat, lon) in [(south, west), (south, east), (north, west), (north, east)] {
            horizontal.entry(float_key(lat)).or_default().push(lon);
            vertical.entry(float_key(lon)).or_default().push(lat);
        }
    }
    if let Some(edges) = regional_edges.as_mut() {
        for (patch, posts) in scene.patches().iter().zip(edges.iter_mut()) {
            let [west, south, east, north] = patch.bounds();
            // Add every adaptive edge endpoint inside the shared perimeter.
            // Copy first: registration adds source posts to the same maps.
            for latitude in [south, north] {
                if let Some(longitudes) = horizontal.get(&float_key(latitude)) {
                    posts.extend(
                        longitudes
                            .iter()
                            .copied()
                            .filter(|longitude| *longitude >= west && *longitude <= east)
                            .map(|longitude| Geodetic::new(latitude, longitude, 0.0).unwrap()),
                    );
                }
            }
            for longitude in [west, east] {
                if let Some(latitudes) = vertical.get(&float_key(longitude)) {
                    posts.extend(
                        latitudes
                            .iter()
                            .copied()
                            .filter(|latitude| *latitude >= south && *latitude <= north)
                            .map(|latitude| Geodetic::new(latitude, longitude, 0.0).unwrap()),
                    );
                }
            }
            for post in posts.iter() {
                if post.lat_deg == south || post.lat_deg == north {
                    horizontal
                        .entry(float_key(post.lat_deg))
                        .or_default()
                        .push(post.lon_deg);
                }
                if post.lon_deg == west || post.lon_deg == east {
                    vertical
                        .entry(float_key(post.lon_deg))
                        .or_default()
                        .push(post.lat_deg);
                }
            }
        }
    }
    for values in horizontal.values_mut().chain(vertical.values_mut()) {
        values.sort_by(f64::total_cmp);
        values.dedup();
    }
    let mut mesh = RenderMesh::default();
    for [west, south, east, north] in cells {
        let center = Geodetic::new((south + north) * 0.5, (west + east) * 0.5, 0.0).unwrap();
        let middle = reference_vertex(&mut mesh, local, center);
        let mut boundary = Vec::new();
        // Counterclockwise as viewed from above the ellipsoid.
        let south_posts = &horizontal[&float_key(south)];
        let east_posts = &vertical[&float_key(east)];
        let north_posts = &horizontal[&float_key(north)];
        let west_posts = &vertical[&float_key(west)];
        boundary.extend(
            south_posts
                .iter()
                .copied()
                .filter(|x| *x >= west && *x < east)
                .map(|x| (south, x)),
        );
        boundary.extend(
            east_posts
                .iter()
                .copied()
                .filter(|y| *y >= south && *y < north)
                .map(|y| (y, east)),
        );
        boundary.extend(
            north_posts
                .iter()
                .rev()
                .copied()
                .filter(|x| *x > west && *x <= east)
                .map(|x| (north, x)),
        );
        boundary.extend(
            west_posts
                .iter()
                .rev()
                .copied()
                .filter(|y| *y > south && *y <= north)
                .map(|y| (y, west)),
        );
        let edge: Vec<u32> = boundary
            .into_iter()
            .map(|(lat, lon)| {
                reference_vertex(&mut mesh, local, Geodetic::new(lat, lon, 0.0).unwrap())
            })
            .collect();
        for index in 0..edge.len() {
            mesh.indices
                .extend([middle, edge[index], edge[(index + 1) % edge.len()]]);
        }
    }
    mesh
}

fn float_key(value: f64) -> u64 {
    // Canonicalize signed zero so cells on the equator/prime meridian share edges.
    if value == 0.0 { 0 } else { value.to_bits() }
}

fn reference_vertex(mesh: &mut RenderMesh, local: LocalFrame, position: Geodetic) -> u32 {
    mesh.vertex(
        local,
        geodetic_to_ecef(position),
        EnuBasis::at(position).up,
        ELLIPSOID_LAND_COLOR,
    )
}

fn refine_cell(bounds: [f64; 4], eye: Vec3d, focal: f64, depth: usize, cells: &mut Vec<[f64; 4]>) {
    let [west, south, east, north] = bounds;
    let center =
        geodetic_to_ecef(Geodetic::new((south + north) * 0.5, (west + east) * 0.5, 0.0).unwrap());
    let angular_width = (east - west).to_radians();
    let angular_height = (north - south).to_radians();
    // Conservative curvature estimate, inflated for the ellipsoid's principal
    // curvature and the sum of lat/lon spans. This controls presentation LOD.
    let angle = angular_width + angular_height;
    let maximum_curvature_radius = WGS84_A_M * WGS84_A_M / WGS84_B_M;
    let radius = maximum_curvature_radius * angle * 0.5;
    let distance = (eye.distance(center) - radius).max(0.05);
    let relative_derivative_bound = 1.5 * WGS84_E2 / (1.0 - WGS84_E2);
    let hessian_bound = maximum_curvature_radius * (1.0 + relative_derivative_bound.powi(2)).sqrt();
    let chord_error = hessian_bound * angle * angle * 0.125;
    if depth < 17 && chord_error * focal / distance > 0.4 {
        let mid_lon = (west + east) * 0.5;
        let mid_lat = (south + north) * 0.5;
        for child in [
            [west, south, mid_lon, mid_lat],
            [mid_lon, south, east, mid_lat],
            [west, mid_lat, mid_lon, north],
            [mid_lon, mid_lat, east, north],
        ] {
            refine_cell(child, eye, focal, depth + 1, cells);
        }
    } else {
        cells.push(bounds);
    }
}

mod grass;
pub use grass::*;
mod boundaries;
pub use boundaries::*;
