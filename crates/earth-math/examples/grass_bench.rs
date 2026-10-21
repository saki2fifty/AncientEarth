//! Local CPU timings, not GPU frame-rate measurements.
use earth_math::rendering::{
    GrassPlacementCache, LocalFrame, grass_intersection_cached, grass_ray_bounds,
    grass_render_mesh, grass_wind,
};
use earth_math::{EnuBasis, TerrainPatch, TerrainScene, ecef_to_geodetic, geodetic_to_ecef};
use std::{hint::black_box, time::Instant};

fn main() {
    let scene = TerrainScene::boundary_only(vec![
        TerrainPatch::from_boundary_bytes(include_bytes!(
            "../../../assets/boundaries/region-04.aeb"
        ))
        .unwrap(),
    ])
    .unwrap();
    let mut observer = scene.boundary_observer().unwrap();
    let location = observer.feet;
    scene.place_on_ground(&mut observer, location).unwrap();
    let local = LocalFrame::at(observer);
    let wind = grass_wind(123.0);
    let mut moved = observer;
    let destination =
        ecef_to_geodetic(geodetic_to_ecef(observer.feet) + EnuBasis::at(observer.feet).east * 4.5)
            .unwrap();
    scene.place_on_ground(&mut moved, destination).unwrap();
    for quality in 0..=3 {
        let mut cache = GrassPlacementCache::default();
        let start = Instant::now();
        let blades = cache.rebuild(&scene, observer, quality);
        let placement = start.elapsed();
        let start = Instant::now();
        let mesh = grass_render_mesh(&blades, local);
        let bounds = grass_ray_bounds(&blades, local);
        let mesh_time = start.elapsed();
        let start = Instant::now();
        for _ in 0..100 {
            black_box(grass_intersection_cached(
                &blades,
                local,
                &bounds,
                wind,
                observer.frame().center_ray(),
            ));
        }
        let ray_mean_ms = start.elapsed().as_secs_f64() * 10.0;
        let start = Instant::now();
        black_box(cache.rebuild(&scene, moved, quality));
        let moved_time = start.elapsed();
        println!(
            "quality={quality} blades={} vertices={} cold_placement_ms={:.3} static_mesh_bounds_ms={:.3} mean_ray_ms={:.3} cached_moved_placement_ms={:.3}",
            blades.len(),
            mesh.mesh.positions.len(),
            placement.as_secs_f64() * 1000.0,
            mesh_time.as_secs_f64() * 1000.0,
            ray_mean_ms,
            moved_time.as_secs_f64() * 1000.0,
        );
    }
}
