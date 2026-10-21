//! Heading-plane upper skyline: exact-model geometric candidates, with an
//! explicitly sampled fallback for masked ellipsoid boundaries.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkylineMethod {
    ReferenceEllipsoidTangent,
    TerrainSectionExtremum,
    NumericalBoundaryEstimate,
}
impl SkylineMethod {
    pub fn description(self) -> &'static str {
        match self {
            Self::ReferenceEllipsoidTangent => "WGS84 ellipsoid tangent",
            Self::TerrainSectionExtremum => "Terrain heading-plane skyline",
            Self::NumericalBoundaryEstimate => "Numerical boundary estimate",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SkylineMeasurement {
    pub hit: SurfaceHit,
    pub heading_deg: f64,
    pub elevation_deg: f64,
    pub method: SkylineMethod,
    /// Coarse fallback search spacing. Narrower mask gaps can remain unresolved.
    pub angular_search_step_deg: f64,
    /// Final local hit/miss bracket; NOT a bound on a missed separate gap,
    /// numerical range error, atmospheric visibility, or source-data error.
    pub angular_bracket_deg: f64,
}

#[derive(Clone, Copy)]
struct Section {
    origin: Vec3d,
    forward: Vec3d,
    up: Vec3d,
    side: Vec3d,
}
impl Section {
    fn ray(self, elevation: f64) -> Ray {
        Ray {
            origin: self.origin,
            direction: self.forward * elevation.cos() + self.up * elevation.sin(),
        }
    }
    fn elevation(self, point: Vec3d) -> f64 {
        let relative = point - self.origin;
        self.up
            .dot(relative)
            .atan2(self.forward.dot(relative).max(0.0))
    }
    fn crosses(self, bounds: Bounds) -> bool {
        let center = (bounds.min + bounds.max) * 0.5 - self.origin;
        let extent = (bounds.max - bounds.min) * 0.5;
        let radius = |axis: Vec3d| {
            axis.x.abs() * extent.x + axis.y.abs() * extent.y + axis.z.abs() * extent.z
        };
        let padding = 1e-6 + 64.0 * f64::EPSILON * self.origin.length().max(WGS84_A_M);
        self.side.dot(center).abs() <= radius(self.side) + padding
            && self.forward.dot(center) + radius(self.forward) >= -padding
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    elevation: f64,
}

impl TerrainPatch {
    fn highest_section_point(&self, section: Section) -> Option<Candidate> {
        let mut result: Option<Candidate> = None;
        let mut stack = [0usize; 64];
        let mut length = 1;
        let tolerance = 1e-7 + 32.0 * f64::EPSILON * section.origin.length().max(WGS84_A_M);
        while length > 0 {
            length -= 1;
            let node = self.nodes[stack[length]];
            if !section.crosses(node.bounds) {
                continue;
            }
            if node.count == 0 {
                stack[length] = node.left;
                stack[length + 1] = node.right;
                length += 2;
                continue;
            }
            for triangle in &self.triangles[node.start..node.start + node.count] {
                let vertices = triangle.vertices.map(|i| self.vertices[i]);
                let signed = vertices.map(|p| section.side.dot(p - section.origin));
                let mut points = [Vec3d::ZERO; 6];
                let mut count = 0;
                for edge in 0..3 {
                    let next = (edge + 1) % 3;
                    if signed[edge].abs() <= tolerance {
                        points[count] = vertices[edge];
                        count += 1;
                    }
                    if (signed[edge] < 0.0 && signed[next] > 0.0)
                        || (signed[edge] > 0.0 && signed[next] < 0.0)
                    {
                        let t = signed[edge] / (signed[edge] - signed[next]);
                        points[count] = vertices[edge] + (vertices[next] - vertices[edge]) * t;
                        count += 1;
                    }
                }
                let mut consider = |point: Vec3d| {
                    let relative = point - section.origin;
                    if section.forward.dot(relative) < -tolerance || relative.length() < 1e-7 {
                        return;
                    }
                    let elevation = section.elevation(point);
                    if result.is_none_or(|previous| elevation > previous.elevation) {
                        result = Some(Candidate { elevation });
                    }
                };
                for point in &points[..count] {
                    consider(*point);
                }
                // Clip the convex section to the forward half-plane. Usually
                // there are two points; coplanar triangles may have three.
                for first in 0..count {
                    for second in first + 1..count {
                        let a = section.forward.dot(points[first] - section.origin);
                        let b = section.forward.dot(points[second] - section.origin);
                        if (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0) {
                            consider(
                                points[first] + (points[second] - points[first]) * (a / (a - b)),
                            );
                        }
                    }
                }
            }
        }
        result
    }
}

/// Tangent of the actual WGS84 section, not a spherical horizon formula.
fn ellipsoid_tangent(section: Section) -> Option<f64> {
    let scale = |v: Vec3d| Vec3d::new(v.x / WGS84_A_M, v.y / WGS84_A_M, v.z / WGS84_B_M);
    let o = scale(section.origin);
    let h = scale(section.forward);
    let u = scale(section.up);
    let norm = o.length();
    let c = (norm - 1.0) * (norm + 1.0);
    if c <= 0.0 {
        return None;
    } // An interior observer has no ellipsoid tangent.
    let oh = o.dot(h);
    let ou = o.dot(u);
    let a = ou * ou - c * u.dot(u);
    let b = oh * ou - c * h.dot(u);
    let constant = oh * oh - c * h.dot(h);
    let discriminant = b * b - a * constant;
    if discriminant < 0.0 || !discriminant.is_finite() {
        return None;
    }
    let q = -b - discriminant.sqrt().copysign(b);
    let roots = if a == 0.0 {
        if b == 0.0 {
            return None;
        }
        [-constant / (2.0 * b); 2]
    } else if q == 0.0 {
        [-b / a; 2]
    } else {
        [q / a, constant / q]
    };
    roots
        .into_iter()
        .map(f64::atan)
        .filter(|elevation| {
            let d = h * elevation.cos() + u * elevation.sin();
            -o.dot(d) / d.dot(d) >= 0.0
        })
        .max_by(f64::total_cmp)
}

impl TerrainScene {
    /// Upper angular skyline in the vertical plane of the current heading.
    /// This is not the farthest visible object or atmospheric visibility.
    ///
    /// Native triangle-section endpoints and a WGS84 tangent are geometric
    /// candidates. If a regional mask hides the tangent, or eyes are inside the
    /// ellipsoid, a finite angular search resolves a local boundary. Its method
    /// explicitly identifies the result as an estimate, not a certified maximum.
    pub fn skyline(&self, observer: Observer) -> Option<SkylineMeasurement> {
        if self.is_boundary_only() {
            let mut measurement = TerrainScene::default().skyline(observer)?;
            measurement.hit = self.classify_ellipsoid_hit(measurement.hit.hit);
            return Some(measurement);
        }
        if !observer.heading_deg.is_finite() {
            return None;
        }
        let enu = EnuBasis::at(observer.feet);
        let heading = observer.heading_deg.to_radians();
        let forward = enu.north * heading.cos() + enu.east * heading.sin();
        let section = Section {
            origin: observer.eye_position(),
            forward,
            up: enu.up,
            side: forward.cross(enu.up),
        };
        if !section.origin.is_finite() {
            return None;
        }
        let candidate = self
            .patches
            .iter()
            .filter_map(|patch| patch.highest_section_point(section))
            .max_by(|a, b| a.elevation.total_cmp(&b.elevation));
        let mut terrain_hit = candidate.and_then(|point| {
            self.intersect(section.ray(point.elevation))
                .map(|hit| (point.elevation, hit, false))
                .or_else(|| {
                    // An exact shared-edge ray can be numerically ambiguous.
                    // Any nudged result is explicitly classified as estimated.
                    // The original heading-plane ray was tested and missed, so
                    // a shifted hit gives a local hit/miss bracket, not a global
                    // bound on all possible separate silhouette intervals.
                    let elevation = point.elevation - 1e-9;
                    self.intersect(section.ray(elevation))
                        .map(|hit| (elevation, hit, true))
                })
        });
        let tangent = ellipsoid_tangent(section);
        let make =
            |elevation: f64, hit: SurfaceHit, method, step: f64, bracket: f64| SkylineMeasurement {
                hit,
                heading_deg: observer.heading_deg.rem_euclid(360.0),
                elevation_deg: elevation.to_degrees(),
                method,
                angular_search_step_deg: step.to_degrees(),
                angular_bracket_deg: bracket.to_degrees(),
            };
        if let Some(tangent_angle) = tangent {
            if let Some((angle, hit, estimated)) = terrain_hit
                && angle >= tangent_angle
            {
                return Some(make(
                    angle,
                    hit,
                    if estimated {
                        SkylineMethod::NumericalBoundaryEstimate
                    } else {
                        SkylineMethod::TerrainSectionExtremum
                    },
                    0.0,
                    if estimated { 1e-9 } else { 0.0 },
                ));
            }
            if let Some(hit) = self.intersect(section.ray(tangent_angle)) {
                return Some(make(
                    tangent_angle,
                    hit,
                    if matches!(hit.kind, SurfaceKind::ReferenceEllipsoid) {
                        SkylineMethod::ReferenceEllipsoidTangent
                    } else {
                        SkylineMethod::NumericalBoundaryEstimate
                    },
                    0.0,
                    0.0,
                ));
            }
        }
        // Only the masked reference contribution remains above the certified
        // terrain-section candidate. Searching from the top finds the highest
        // sampled hit. A narrower separate unmasked gap may be missed.
        let upper = tangent.unwrap_or(PI * 0.5);
        let lower = terrain_hit.map_or(-PI * 0.5, |(angle, _, _)| angle);
        if lower >= upper {
            return terrain_hit.map(|(angle, hit, estimated)| {
                make(
                    angle,
                    hit,
                    if estimated {
                        SkylineMethod::NumericalBoundaryEstimate
                    } else {
                        SkylineMethod::TerrainSectionExtremum
                    },
                    0.0,
                    if estimated { 1e-9 } else { 0.0 },
                )
            });
        }
        let step = (upper - lower) / 2048.0;
        let mut previous = upper;
        for index in 0..=2048 {
            let angle = upper - index as f64 * step;
            if let Some(hit) = self.intersect(section.ray(angle)) {
                let mut lo = angle;
                let mut hi = previous;
                let mut selected = hit;
                for _ in 0..40 {
                    if hi - lo <= 1e-11 {
                        break;
                    }
                    let middle = (lo + hi) * 0.5;
                    if let Some(hit) = self.intersect(section.ray(middle)) {
                        lo = middle;
                        selected = hit;
                    } else {
                        hi = middle;
                    }
                }
                return Some(make(
                    lo,
                    selected,
                    SkylineMethod::NumericalBoundaryEstimate,
                    step,
                    hi - lo,
                ));
            }
            previous = angle;
        }
        terrain_hit.take().map(|(angle, hit, _)| {
            make(
                angle,
                hit,
                SkylineMethod::NumericalBoundaryEstimate,
                step,
                step,
            )
        })
    }
}
