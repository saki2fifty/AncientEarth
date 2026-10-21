//! Canonical Earth silhouette guides and explicitly defined world-space rulers.
//! The finite contour search is an estimate, not a certified global maximum.
use crate::{CameraFrame, Geodetic, Lens, Observer, Ray, ScreenRect, SurfaceHit, TerrainScene};
use std::{cell::RefCell, collections::HashMap};

const COLUMNS: usize = 64;
const ROWS: usize = 36;
const BRACKET_PIXELS: f64 = 0.025;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuideViewport {
    pub display: [f32; 2],
    pub scale: [f32; 2],
    pub pixels: [f64; 2],
}
impl GuideViewport {
    pub fn new(display: [f32; 2], scale: [f32; 2]) -> Option<Self> {
        if !display
            .into_iter()
            .chain(scale)
            .all(|v| v.is_finite() && v > 0.0)
        {
            return None;
        }
        let pixels = std::array::from_fn(|i| f64::from(display[i]) * f64::from(scale[i]));
        if !pixels.iter().all(|p| (1.0..=16_777_216.0).contains(p)) {
            return None;
        }
        Some(Self {
            display,
            scale,
            pixels,
        })
    }
    pub fn logical(self, point: [f64; 2]) -> [f32; 2] {
        std::array::from_fn(|i| (point[i] / f64::from(self.scale[i])) as f32)
    }
    fn contains(self, point: [f64; 2]) -> bool {
        (0..2).all(|i| point[i].is_finite() && (0.0..=self.pixels[i]).contains(&point[i]))
    }
    fn direction(self, frame: CameraFrame, lens: Lens, point: [f64; 2]) -> Option<crate::Vec3d> {
        self.contains(point).then_some(())?;
        lens.ray(
            frame,
            2.0 * point[0] / self.pixels[0] - 1.0,
            1.0 - 2.0 * point[1] / self.pixels[1],
            self.pixels[0] / self.pixels[1],
        )
    }
    fn project(self, frame: CameraFrame, lens: Lens, point: crate::Vec3d) -> Option<[f64; 2]> {
        let ndc = lens.project(frame, point - frame.origin, self.pixels[0] / self.pixels[1])?;
        Some([
            (ndc[0] + 1.0) * self.pixels[0] * 0.5,
            (1.0 - ndc[1]) * self.pixels[1] * 0.5,
        ])
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HorizonPoint {
    /// Physical framebuffer coordinates, including subpixel refinement.
    pub pixel: [f64; 2],
    pub hit: SurfaceHit,
    /// Width of this local hit/miss bracket, not a global error bound.
    pub bracket_pixels: f64,
}

#[derive(Clone, Debug)]
pub struct HorizonProfile {
    pub apex: HorizonPoint,
    pub left: HorizonPoint,
    pub right: HorizonPoint,
    pub chord_width_m: Option<f64>,
    /// Shortest WGS84 ellipsoid geodesic between the selected endpoints'
    /// latitudes and longitudes; terrain heights are projected to zero.
    pub surface_width_m: Option<f64>,
    pub coarse_spacing_pixels: [f64; 2],
    pub viewport: GuideViewport,
    frame: CameraFrame,
    lens: Lens,
    atmosphere: Option<crate::atmosphere::AtmosphereProfile>,
}

#[derive(Clone, Copy, Debug)]
pub struct GuideRuler {
    pub ground: HorizonPoint,
    /// Constructed guide reference at the measured ground's camera depth.
    /// This is not a ray hit in the sky or a geodetic height reference.
    pub reference_ecef_m: crate::Vec3d,
    pub view_plane_gap_m: f64,
    /// Euclidean eye-to-selected-endpoint chord, including in refracted mode;
    /// this is neither the curved geometric path nor the phase optical path.
    pub eye_range_m: f64,
    pub top_pixel: [f64; 2],
}

#[derive(Clone, Copy)]
struct Sample {
    pixel: [f64; 2],
    hit: bool,
}
#[derive(Clone, Copy)]
struct Boundary {
    pixel: [f64; 2],
    bracket_pixels: f64,
}
struct Search<'a> {
    scene: &'a TerrainScene,
    atmosphere: Option<&'a crate::atmosphere::AtmosphereProfile>,
    frame: CameraFrame,
    lens: Lens,
    viewport: GuideViewport,
    samples: RefCell<HashMap<[u64; 2], Option<Sample>>>,
}
impl Search<'_> {
    fn sample(&self, pixel: [f64; 2]) -> Option<Sample> {
        let key = pixel.map(f64::to_bits);
        if let Some(sample) = self.samples.borrow().get(&key) {
            return *sample;
        }
        let sample = self
            .viewport
            .direction(self.frame, self.lens, pixel)
            .and_then(|direction| {
                let ray = Ray {
                    origin: self.frame.origin,
                    direction,
                };
                let hit = if let Some(profile) = self.atmosphere {
                    // An unresolved ray is not a valid sky sample or a contour.
                    profile.trace(ray, self.scene).ok()?.surface.is_some()
                } else {
                    self.scene.intersects_any(ray)
                };
                Some(Sample { pixel, hit })
            });
        self.samples.borrow_mut().insert(key, sample);
        sample
    }
    fn surface(&self, pixel: [f64; 2]) -> Option<SurfaceHit> {
        let direction = self.viewport.direction(self.frame, self.lens, pixel)?;
        let ray = Ray {
            origin: self.frame.origin,
            direction,
        };
        if let Some(profile) = self.atmosphere {
            profile.trace(ray, self.scene).ok()?.surface
        } else {
            self.scene.intersect(ray)
        }
    }
    fn resolved(&self, boundary: Boundary) -> Option<HorizonPoint> {
        Some(HorizonPoint {
            pixel: boundary.pixel,
            bracket_pixels: boundary.bracket_pixels,
            hit: self.surface(boundary.pixel)?,
        })
    }
    fn boundary(&self, a: Sample, b: Sample) -> Option<Boundary> {
        if a.hit == b.hit {
            return None;
        }
        let (mut hit, mut miss) = if a.hit { (a, b) } else { (b, a) };
        for _ in 0..40 {
            if pixel_distance(hit.pixel, miss.pixel) <= BRACKET_PIXELS {
                break;
            }
            let middle = std::array::from_fn(|i| (hit.pixel[i] + miss.pixel[i]) * 0.5);
            let sample = self.sample(middle)?;
            if sample.hit {
                hit = sample;
            } else {
                miss = sample;
            }
        }
        Some(Boundary {
            pixel: hit.pixel,
            bracket_pixels: pixel_distance(hit.pixel, miss.pixel),
        })
    }
    fn segment_boundaries(&self, a: [f64; 2], b: [f64; 2], steps: usize) -> Vec<Boundary> {
        let mut result = Vec::new();
        let mut previous = None;
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let point = std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t);
            let sample = self.sample(point);
            if let (Some(old), Some(new)) = (previous, sample)
                && let Some(boundary) = self.boundary(old, new)
            {
                result.push(boundary);
            }
            previous = sample;
        }
        result
    }
    fn refine_extreme(&self, mut best: Boundary, axis: usize, minimize: bool) -> Boundary {
        let tangent_axis = 1 - axis;
        let mut step = self.viewport.pixels[tangent_axis]
            / if tangent_axis == 0 {
                COLUMNS as f64
            } else {
                ROWS as f64
            };
        let span = self.viewport.pixels[axis]
            / if axis == 0 {
                COLUMNS as f64
            } else {
                ROWS as f64
            };
        for _ in 0..12 {
            for sign in [-1.0, 1.0] {
                let mut a = best.pixel;
                a[tangent_axis] += sign * step;
                a[axis] = (best.pixel[axis] - span).max(0.0);
                let mut b = a;
                b[axis] = (best.pixel[axis] + span).min(self.viewport.pixels[axis]);
                for point in self.segment_boundaries(a, b, 4) {
                    let improves = if minimize {
                        point.pixel[axis] < best.pixel[axis]
                    } else {
                        point.pixel[axis] > best.pixel[axis]
                    };
                    if improves {
                        best = point;
                    }
                }
            }
            step *= 0.5;
        }
        best
    }
}

fn pixel_distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl HorizonProfile {
    /// Find the visible canonical Earth outline, excluding grass, bodies and UI.
    /// Coarse screen cells can miss isolated narrow features; no global maximum
    /// guarantee is inferred from the much smaller local bisection bracket.
    pub fn measure(
        scene: &TerrainScene,
        frame: CameraFrame,
        lens: Lens,
        viewport: GuideViewport,
    ) -> Option<Self> {
        Self::measure_with_atmosphere(scene, frame, lens, viewport, None)
    }
    /// Same finite contour search, using the validated optical profile for each ray.
    pub fn measure_refracted(
        scene: &TerrainScene,
        frame: CameraFrame,
        lens: Lens,
        viewport: GuideViewport,
        profile: &crate::atmosphere::AtmosphereProfile,
    ) -> Option<Self> {
        Self::measure_with_atmosphere(scene, frame, lens, viewport, Some(profile))
    }
    pub fn measure_with_atmosphere(
        scene: &TerrainScene,
        frame: CameraFrame,
        lens: Lens,
        viewport: GuideViewport,
        atmosphere: Option<&crate::atmosphere::AtmosphereProfile>,
    ) -> Option<Self> {
        if !frame.origin.is_finite() {
            return None;
        }
        let search = Search {
            scene,
            atmosphere,
            frame,
            lens,
            viewport,
            samples: RefCell::default(),
        };
        let mut points = Vec::new();
        let mut previous_row = vec![None; COLUMNS + 1];
        for row in 0..=ROWS {
            let mut previous = None;
            for (column, above) in previous_row.iter_mut().enumerate() {
                let sample = search.sample([
                    viewport.pixels[0] * column as f64 / COLUMNS as f64,
                    viewport.pixels[1] * row as f64 / ROWS as f64,
                ]);
                for neighbor in [previous, *above] {
                    if let (Some(a), Some(b)) = (neighbor, sample)
                        && let Some(point) = search.boundary(a, b)
                    {
                        points.push(point);
                    }
                }
                previous = sample;
                *above = sample;
            }
        }
        // Seed sharp terrain summits directly from canonical posts. This avoids
        // relying exclusively on the coarse image lattice for narrow peaks.
        let mut summits: Vec<[f64; 2]> = Vec::with_capacity(8);
        for patch in scene.patches().iter().filter(|_| !scene.is_boundary_only()) {
            for &vertex in patch.canonical_vertices() {
                if let Some(pixel) = viewport.project(frame, lens, vertex)
                    && (summits.len() < 8 || pixel[1] < summits[7][1])
                {
                    let index = summits.partition_point(|p| p[1] <= pixel[1]);
                    summits.insert(index, pixel);
                    summits.truncate(8);
                }
            }
        }
        let vertical_step = viewport.pixels[1] / ROWS as f64;
        for summit in summits {
            points.extend(search.segment_boundaries(
                [summit[0], (summit[1] - vertical_step).max(0.0)],
                [summit[0], (summit[1] + 0.1).min(viewport.pixels[1])],
                4,
            ));
        }
        let apex = *points
            .iter()
            .min_by(|a, b| a.pixel[1].total_cmp(&b.pixel[1]))?;
        let left = *points
            .iter()
            .min_by(|a, b| a.pixel[0].total_cmp(&b.pixel[0]))?;
        let right = *points
            .iter()
            .max_by(|a, b| a.pixel[0].total_cmp(&b.pixel[0]))?;
        let apex = search.resolved(search.refine_extreme(apex, 1, true))?;
        let left = search.resolved(search.refine_extreme(left, 0, true))?;
        let right = search.resolved(search.refine_extreme(right, 0, false))?;
        let chord_width_m = (right.pixel[0] - left.pixel[0] > BRACKET_PIXELS)
            .then(|| left.hit.hit.position.distance(right.hit.hit.position));
        let surface_width_m = chord_width_m.and_then(|_| {
            crate::wgs84_surface_distance(left.hit.hit.geodetic, right.hit.hit.geodetic).ok()
        });
        Some(Self {
            apex,
            left,
            right,
            chord_width_m,
            surface_width_m,
            viewport,
            frame,
            lens,
            coarse_spacing_pixels: [viewport.pixels[0] / COLUMNS as f64, vertical_step],
            atmosphere: atmosphere.cloned(),
        })
    }
    pub fn ruler(&self, scene: &TerrainScene, column: f64) -> Option<GuideRuler> {
        self.ruler_with_atmosphere(scene, column, self.atmosphere.as_ref())
    }
    pub fn ruler_refracted(
        &self,
        scene: &TerrainScene,
        column: f64,
        profile: &crate::atmosphere::AtmosphereProfile,
    ) -> Option<GuideRuler> {
        self.ruler_with_atmosphere(scene, column, Some(profile))
    }
    pub fn ruler_with_atmosphere(
        &self,
        scene: &TerrainScene,
        column: f64,
        atmosphere: Option<&crate::atmosphere::AtmosphereProfile>,
    ) -> Option<GuideRuler> {
        if self.atmosphere.as_ref().map(|profile| profile.layers())
            != atmosphere.map(|profile| profile.layers())
        {
            return None;
        }
        if !column.is_finite() || !(0.0..=1.0).contains(&column) {
            return None;
        }
        let x = column * self.viewport.pixels[0];
        let search = Search {
            scene,
            atmosphere,
            frame: self.frame,
            lens: self.lens,
            viewport: self.viewport,
            samples: RefCell::default(),
        };
        let mut rows: Vec<f64> = (0..=128)
            .map(|row| self.viewport.pixels[1] * row as f64 / 128.0)
            .collect();
        // A thin terrain band may fall between coarse rows. Probe the known
        // global summit's height explicitly; every seed still needs a real
        // hit at this column, rather than assuming the summit extends here.
        for offset in [-0.1, -BRACKET_PIXELS, 0.0, BRACKET_PIXELS, 0.1] {
            rows.push((self.apex.pixel[1] + offset).clamp(0.0, self.viewport.pixels[1]));
        }
        rows.sort_by(f64::total_cmp);
        rows.dedup();
        let mut previous: Option<Sample> = None;
        let mut ground = None;
        for row in rows {
            let current = search.sample([x, row]);
            if let (Some(sky), Some(earth)) = (previous, current)
                && !sky.hit
                && earth.hit
            {
                ground = search.boundary(sky, earth);
                break;
            }
            previous = current;
        }
        let mut ground = search.resolved(ground?)?;
        // A different column may reveal a missed summit above the sampled apex.
        // Never draw a reversed ruler or silently promote it to a global bound.
        if ground.pixel[1] < self.apex.pixel[1] {
            let expanded = ground.bracket_pixels + self.apex.pixel[1] - ground.pixel[1];
            if expanded > BRACKET_PIXELS {
                return None;
            }
            // Preserve the original sky endpoint and extend the local bracket
            // to an actual hit at the guide. This reconciles independently
            // refined subpixel crossings without drawing an upward ruler.
            let pixel = [x, self.apex.pixel[1]];
            ground = HorizonPoint {
                pixel,
                hit: search.surface(pixel)?,
                bracket_pixels: expanded,
            };
        }
        let top_pixel = [x, self.apex.pixel[1]];
        let direction = self.viewport.direction(self.frame, self.lens, top_pixel)?;
        let depth = (ground.hit.hit.position - self.frame.origin).dot(self.frame.forward);
        let denominator = direction.dot(self.frame.forward);
        if depth <= 0.0 || denominator <= 1e-10 {
            return None;
        }
        let reference_ecef_m = if let Some(profile) = atmosphere {
            profile
                .trace_to_plane(
                    Ray {
                        origin: self.frame.origin,
                        direction,
                    },
                    scene,
                    ground.hit.hit.position,
                    self.frame.forward,
                )
                .ok()??
        } else {
            self.frame.origin + direction * (depth / denominator)
        };
        let view_plane_gap_m = reference_ecef_m.distance(ground.hit.hit.position);
        if !view_plane_gap_m.is_finite() {
            return None;
        }
        Some(GuideRuler {
            ground,
            reference_ecef_m,
            view_plane_gap_m,
            eye_range_m: ground.hit.hit.distance_m,
            top_pixel,
        })
    }
    /// Filled, non-antialiased row, exactly one physical framebuffer pixel.
    pub fn horizontal_rect(&self) -> ScreenRect {
        let row = self.apex.pixel[1]
            .floor()
            .clamp(0.0, self.viewport.pixels[1].floor() - 1.0);
        ScreenRect {
            min: self.viewport.logical([0.0, row]),
            max: self.viewport.logical([self.viewport.pixels[0], row + 1.0]),
        }
    }
    pub fn vertical_rect(&self, ruler: GuideRuler) -> ScreenRect {
        let column = ruler.top_pixel[0]
            .floor()
            .clamp(0.0, self.viewport.pixels[0].floor() - 1.0);
        ScreenRect {
            min: self.viewport.logical([column, ruler.top_pixel[1]]),
            max: self.viewport.logical([column + 1.0, ruler.ground.pixel[1]]),
        }
    }
    pub fn width_label_position(&self) -> [f32; 2] {
        let apex = self.viewport.logical(self.apex.pixel);
        [
            ((self.viewport.display[0] - 440.0) * 0.5).max(12.0),
            (apex[1] - 34.0).max(4.0),
        ]
    }
    pub fn ruler_label_position(&self, ruler: GuideRuler) -> [f32; 2] {
        let top = self.viewport.logical(ruler.top_pixel);
        let bottom = self.viewport.logical(ruler.ground.pixel);
        [
            (top[0] + 10.0).min((self.viewport.display[0] - 360.0).max(4.0)),
            ((top[1] + bottom[1]) * 0.5 + 8.0).min((self.viewport.display[1] - 40.0).max(4.0)),
        ]
    }
    pub fn marker_position(&self, column: f64) -> [f32; 2] {
        self.viewport.logical([
            column.clamp(0.0, 1.0) * self.viewport.pixels[0],
            self.apex.pixel[1],
        ])
    }
}

pub fn move_column(current: f64, axis: f64, elapsed_seconds: f64) -> f64 {
    if !current.is_finite() {
        return 0.5;
    }
    if !axis.is_finite() || !elapsed_seconds.is_finite() {
        return current.clamp(0.0, 1.0);
    }
    (current + axis.clamp(-1.0, 1.0) * elapsed_seconds.clamp(0.0, 0.25) * 0.35).clamp(0.0, 1.0)
}
pub fn column_from_screen(mouse_x: f32, width: f32) -> Option<f64> {
    (mouse_x.is_finite() && width.is_finite() && width > 0.0)
        .then(|| (f64::from(mouse_x) / f64::from(width)).clamp(0.0, 1.0))
}

#[derive(Clone, Copy, Debug)]
pub enum GuideDemo {
    HighAltitude,
    Rolled,
    SkyOnly,
}
pub fn demo_observer(demo: GuideDemo) -> Observer {
    let mut observer = Observer::new(Geodetic::new(0.0, 0.0, 1_000_000.0).unwrap());
    observer.heading_deg = 90.0;
    observer.pitch_deg = if matches!(demo, GuideDemo::SkyOnly) {
        90.0
    } else {
        -30.0
    };
    observer.roll_deg = if matches!(demo, GuideDemo::Rolled) {
        30.0
    } else {
        0.0
    };
    observer
}
