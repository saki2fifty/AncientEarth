//! Shared h=0 display edges at the retained classification-region perimeter.
//! The physical surface remains the analytic ellipsoid, independent of this mesh.
use super::{
    ELLIPSOID_LAND_COLOR, LocalFrame, RenderMesh, reference_mesh_impl, terrain_water_mesh,
};
use crate::{EnuBasis, Geodetic, TerrainPatch, TerrainScene, Vec3d, geodetic_to_ecef};
use std::collections::HashMap;

pub struct RegionMeshes {
    pub land: RenderMesh,
    pub water: RenderMesh,
    pub stride: usize,
}

pub struct BoundaryMeshes {
    pub reference: RenderMesh,
    pub regions: Vec<RegionMeshes>,
}

/// Builds the global and regional display partitions together. The union of
/// source and adaptive perimeter posts is used on both sides of each boundary.
/// Legacy terrain scenes retain their original independent presentation path.
pub fn boundary_scene_meshes(
    scene: &TerrainScene,
    local: LocalFrame,
    focal: f64,
) -> BoundaryMeshes {
    if !scene.is_boundary_only() {
        return BoundaryMeshes {
            reference: reference_mesh_impl(scene, local, focal, None),
            regions: scene
                .patches()
                .iter()
                .map(|patch| {
                    let (land, water, stride) = terrain_water_mesh(patch, local, focal);
                    RegionMeshes {
                        land,
                        water,
                        stride,
                    }
                })
                .collect(),
        };
    }
    let mut edges: Vec<Vec<Geodetic>> = scene
        .patches()
        .iter()
        .map(|patch| {
            let [west, south, east, north] = patch.bounds();
            (0..patch.canonical_vertices().len())
                .filter_map(|index| {
                    let point = patch.boundary_geodetic(index)?;
                    (point.lat_deg == south
                        || point.lat_deg == north
                        || point.lon_deg == west
                        || point.lon_deg == east)
                        .then_some(point)
                })
                .collect()
        })
        .collect();
    let reference = reference_mesh_impl(scene, local, focal, Some(&mut edges));
    let regions = scene
        .patches()
        .iter()
        .zip(edges)
        .map(|(patch, edges)| boundary_patch_mesh(patch, local, &edges))
        .collect();
    BoundaryMeshes { reference, regions }
}

fn point_key(point: Vec3d) -> [u64; 3] {
    [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()]
}

fn boundary_patch_mesh(
    patch: &TerrainPatch,
    local: LocalFrame,
    posts: &[Geodetic],
) -> RegionMeshes {
    let [west, south, east, north] = patch.bounds();
    let mut south_edge = Vec::new();
    let mut north_edge = Vec::new();
    let mut west_edge = Vec::new();
    let mut east_edge = Vec::new();
    for post in posts {
        if post.lat_deg == south {
            south_edge.push(post.lon_deg);
        }
        if post.lat_deg == north {
            north_edge.push(post.lon_deg);
        }
        if post.lon_deg == west {
            west_edge.push(post.lat_deg);
        }
        if post.lon_deg == east {
            east_edge.push(post.lat_deg);
        }
    }
    for edge in [
        &mut south_edge,
        &mut north_edge,
        &mut west_edge,
        &mut east_edge,
    ] {
        edge.sort_by(f64::total_cmp);
        edge.dedup();
    }
    let lookup: HashMap<_, _> = patch
        .canonical_vertices()
        .iter()
        .enumerate()
        .map(|(index, &point)| (point_key(point), index))
        .collect();
    let mut meshes = [RenderMesh::default(), RenderMesh::default()];
    let mut remaps: [HashMap<[u64; 3], u32>; 2] = Default::default();
    for (triangle, water) in patch.presentation_triangles() {
        let material = usize::from(water.is_some());
        let points = triangle.map(|point| {
            patch
                .boundary_geodetic(lookup[&point_key(point)])
                .expect("boundary-only scene supplies h=0 source parameters")
        });
        let mut polygon = Vec::with_capacity(3);
        for edge in 0..3 {
            let a = points[edge];
            let b = points[(edge + 1) % 3];
            polygon.push(a);
            let (values, start, end, constant, horizontal) =
                if a.lat_deg == b.lat_deg && a.lat_deg == south {
                    (&south_edge, a.lon_deg, b.lon_deg, south, true)
                } else if a.lat_deg == b.lat_deg && a.lat_deg == north {
                    (&north_edge, a.lon_deg, b.lon_deg, north, true)
                } else if a.lon_deg == b.lon_deg && a.lon_deg == west {
                    (&west_edge, a.lat_deg, b.lat_deg, west, false)
                } else if a.lon_deg == b.lon_deg && a.lon_deg == east {
                    (&east_edge, a.lat_deg, b.lat_deg, east, false)
                } else {
                    continue;
                };
            let lo = values.partition_point(|value| *value <= start.min(end));
            let hi = values.partition_point(|value| *value < start.max(end));
            let mut inserted: Vec<_> = values[lo..hi]
                .iter()
                .map(|&value| {
                    if horizontal {
                        Geodetic::new(constant, value, 0.0).unwrap()
                    } else {
                        Geodetic::new(value, constant, 0.0).unwrap()
                    }
                })
                .collect();
            if start > end {
                inserted.reverse();
            }
            polygon.extend(inserted);
        }
        let color = if material == 1 {
            [1.0; 4]
        } else {
            ELLIPSOID_LAND_COLOR
        };
        let mut vertex = |position: Geodetic| {
            let point = geodetic_to_ecef(position);
            *remaps[material].entry(point_key(point)).or_insert_with(|| {
                meshes[material].vertex(local, point, EnuBasis::at(position).up, color)
            })
        };
        if polygon.len() == 3 {
            let indices = [vertex(points[0]), vertex(points[1]), vertex(points[2])];
            meshes[material].indices.extend(indices);
        } else {
            // A geographic triangle is convex in its latitude/longitude
            // parameter plane; its centroid and ordered subdivided edges form
            // disjoint positive-area fans, retaining the original material.
            let center = Geodetic::new(
                points.iter().map(|p| p.lat_deg).sum::<f64>() / 3.0,
                points.iter().map(|p| p.lon_deg).sum::<f64>() / 3.0,
                0.0,
            )
            .unwrap();
            let middle = vertex(center);
            let edge: Vec<_> = polygon.into_iter().map(vertex).collect();
            for index in 0..edge.len() {
                meshes[material].indices.extend([
                    middle,
                    edge[index],
                    edge[(index + 1) % edge.len()],
                ]);
            }
        }
    }
    let [land, water] = meshes;
    RegionMeshes {
        land,
        water,
        stride: 1,
    }
}
