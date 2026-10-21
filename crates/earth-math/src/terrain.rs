//! Active height-free categories on analytic WGS84, plus archived triangle
//! surface routines retained for historical tests. See docs/science/water.md.
use super::*;
mod boundary;
mod skyline;
pub use skyline::{SkylineMeasurement, SkylineMethod};

#[derive(Clone, Debug, PartialEq)]
pub enum TerrainError {
    InvalidFormat,
    InvalidGrid,
    InvalidPosition(GeometryError),
    MissingTerrain,
    BelowSurface,
}
impl fmt::Display for TerrainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidFormat => "Invalid or unsupported AncientEarth terrain file.",
            Self::InvalidGrid => {
                "Terrain grid needs finite, registered heights and at least two rows and columns."
            }
            Self::InvalidPosition(_) => {
                "Coordinates or eye height are outside the supported domain."
            }
            Self::MissingTerrain => "No valid terrain surface is available at this position.",
            Self::BelowSurface => "Requested feet height is below the active terrain surface.",
        })
    }
}
impl std::error::Error for TerrainError {}
impl From<GeometryError> for TerrainError {
    fn from(value: GeometryError) -> Self {
        Self::InvalidPosition(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterKind {
    Ocean,
    Lake,
    River,
}
impl WaterKind {
    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Ocean),
            2 => Some(Self::Lake),
            3 => Some(Self::River),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Terrain(usize),
    Water { patch: usize, kind: WaterKind },
    ReferenceEllipsoid,
}
#[derive(Clone, Copy, Debug)]
pub struct SurfaceHit {
    pub hit: RayHit,
    pub kind: SurfaceKind,
    /// False on classified water and on conservatively excluded mixed shores.
    pub grass_allowed: bool,
}
fn component(p: Vec3d, axis: usize) -> f64 {
    match axis {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: Vec3d,
    max: Vec3d,
}
impl Bounds {
    fn empty() -> Self {
        Self {
            min: Vec3d::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Vec3d::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
    fn include(&mut self, p: Vec3d) {
        self.min = Vec3d::new(
            self.min.x.min(p.x),
            self.min.y.min(p.y),
            self.min.z.min(p.z),
        );
        self.max = Vec3d::new(
            self.max.x.max(p.x),
            self.max.y.max(p.y),
            self.max.z.max(p.z),
        );
    }
    /// Conservative slabs: expanded spatial bounds plus relative t rounding.
    fn entry(self, origin: Vec3d, direction: Vec3d, limit: f64) -> Option<f64> {
        let mut near = 0.0_f64;
        let mut far = limit;
        let padding = 1e-6 + 32.0 * f64::EPSILON * origin.length().max(WGS84_A_M);
        for axis in 0..3 {
            let o = component(origin, axis);
            let d = component(direction, axis);
            let lo = component(self.min, axis) - padding;
            let hi = component(self.max, axis) + padding;
            if d == 0.0 {
                if o < lo || o > hi {
                    return None;
                }
            } else {
                let a = (lo - o) / d;
                let b = (hi - o) / d;
                near = near.max(a.min(b));
                far = far.min(a.max(b));
                if near > far {
                    return None;
                }
            }
        }
        Some(near)
    }
}

#[derive(Clone, Copy, Debug)]
struct Triangle {
    vertices: [usize; 3],
    normal: Vec3d,
    center: Vec3d,
    // 0 land, 1 ocean, 2 lake, 3 river, 4 mixed (no grass).
    water_code: u8,
}
impl Triangle {
    fn checked(
        vertices: &[Vec3d],
        indices: [usize; 3],
        water_code: u8,
    ) -> Result<Self, TerrainError> {
        if water_code > 4 || indices.iter().any(|index| *index >= vertices.len()) {
            return Err(TerrainError::InvalidGrid);
        }
        let [a, b, c] = indices.map(|index| vertices[index]);
        let mut normal = (b - a)
            .cross(c - a)
            .normalized()
            .ok_or(TerrainError::InvalidGrid)?;
        let center = (a + b + c) / 3.0;
        if normal.dot(center) < 0.0 {
            normal = -normal;
        }
        let center_geodetic =
            inverse_near_surface(center).map_err(|_| TerrainError::InvalidGrid)?;
        let up = EnuBasis::at(center_geodetic).up;
        let support = WGS84_A_M * (1.0 - WGS84_E2 * up.z * up.z).sqrt();
        if [a, b, c]
            .into_iter()
            .any(|vertex| vertex.dot(up) - support < MIN_HEIGHT_M)
        {
            return Err(TerrainError::InvalidGrid);
        }
        Ok(Self {
            vertices: indices,
            normal,
            center,
            water_code,
        })
    }
}
#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Bounds,
    left: usize,
    right: usize,
    start: usize,
    count: usize,
}

/// Regular geographic posts converted to a fixed NW-SE diagonal ECEF mesh.
/// Input heights must already be WGS84 ellipsoidal metres (h = H + geoid N).
#[derive(Clone, Debug)]
pub struct TerrainPatch {
    width: usize,
    height: usize,
    west: f64,
    north: f64,
    step_lon: f64,
    step_lat: f64,
    max_height: f64,
    vertices: Vec<Vec3d>,
    triangles: Vec<Triangle>,
    nodes: Vec<Node>,
    refined: bool,
    boundary: Option<boundary::BoundaryData>,
}
impl TerrainPatch {
    #[allow(clippy::too_many_arguments)]
    pub fn from_grid(
        width: usize,
        height: usize,
        west: f64,
        north: f64,
        step_lon: f64,
        step_lat: f64,
        heights: Vec<f64>,
    ) -> Result<Self, TerrainError> {
        let count = width.checked_mul(height).ok_or(TerrainError::InvalidGrid)?;
        if width < 2
            || height < 2
            || count > 4_000_000
            || heights.len() != count
            || ![west, north, step_lon, step_lat]
                .iter()
                .all(|v| v.is_finite())
            || step_lon <= 0.0
            || step_lat <= 0.0
            || !heights
                .iter()
                .all(|v| v.is_finite() && (MIN_HEIGHT_M..=100_000.0).contains(v))
        {
            return Err(TerrainError::InvalidGrid);
        }
        let east = west + (width - 1) as f64 * step_lon;
        let south = north - (height - 1) as f64 * step_lat;
        Geodetic::new(north, west, 0.0)?;
        Geodetic::new(south, east, 0.0)?;
        // This patch format intentionally excludes date-line and polar wraps.
        if east - west > 5.0 || north - south > 5.0 || south <= -89.0 || north >= 89.0 {
            return Err(TerrainError::InvalidGrid);
        }
        let max_height = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut vertices = Vec::with_capacity(count);
        for row in 0..height {
            for col in 0..width {
                vertices.push(geodetic_to_ecef(Geodetic::new(
                    north - row as f64 * step_lat,
                    west + col as f64 * step_lon,
                    heights[row * width + col],
                )?));
            }
        }
        let mut triangles = Vec::with_capacity((width - 1) * (height - 1) * 2);
        for row in 0..height - 1 {
            for col in 0..width - 1 {
                let nw = row * width + col;
                let ne = nw + 1;
                let sw = nw + width;
                let se = sw + 1;
                for indices in [[nw, sw, se], [nw, se, ne]] {
                    // The supported inward parallel surface is convex. A
                    // separating support plane proves every point of this
                    // Cartesian triangle remains outside that lower domain.
                    // Vertex heights alone do not: chords dip below their posts.
                    triangles.push(Triangle::checked(&vertices, indices, 0)?);
                }
            }
        }
        let mut patch = Self {
            width,
            height,
            west,
            north,
            step_lon,
            step_lat,
            max_height,
            vertices,
            triangles,
            nodes: Vec::new(),
            refined: false,
            boundary: None,
        };
        patch.build_node(0, patch.triangles.len());
        Ok(patch)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TerrainError> {
        if bytes.len() < 48 || &bytes[..8] != b"AETERR01" {
            return Err(TerrainError::InvalidFormat);
        }
        let u32_at =
            |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let f64_at = |offset| f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        let width = u32_at(8);
        let height = u32_at(12);
        let count = width
            .checked_mul(height)
            .ok_or(TerrainError::InvalidFormat)?;
        if count > 4_000_000
            || count.checked_mul(8).and_then(|x| x.checked_add(48)) != Some(bytes.len())
        {
            return Err(TerrainError::InvalidFormat);
        }
        Self::from_grid(
            width,
            height,
            f64_at(16),
            f64_at(24),
            f64_at(32),
            f64_at(40),
            (0..count).map(|i| f64_at(48 + 8 * i)).collect(),
        )
    }
    pub fn bounds(&self) -> [f64; 4] {
        [
            self.west,
            self.north - (self.height - 1) as f64 * self.step_lat,
            self.west + (self.width - 1) as f64 * self.step_lon,
            self.north,
        ]
    }
    /// Attach reviewed water labels or a source-backed refined shoreline mesh.
    /// The companion is bound to the exact terrain bytes by an accidental-error
    /// checksum; redistribution provenance separately uses SHA-256.
    pub fn from_bytes_with_water(terrain: &[u8], water: &[u8]) -> Result<Self, TerrainError> {
        let mut patch = Self::from_bytes(terrain)?;
        if water.len() < 56
            || (&water[..8] != b"AEWATR01" && &water[..8] != b"AEWATR02")
            || water[8..48] != terrain[8..48]
        {
            return Err(TerrainError::InvalidFormat);
        }
        let checksum = terrain.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        if u64::from_le_bytes(water[48..56].try_into().unwrap()) != checksum {
            return Err(TerrainError::InvalidFormat);
        }
        if &water[..8] == b"AEWATR02" {
            return patch.with_refined_water(water);
        }
        if water.len() != 56 + patch.triangle_count() || water[56..].iter().any(|code| *code > 4) {
            return Err(TerrainError::InvalidFormat);
        }
        for triangle in &mut patch.triangles {
            let nw = triangle.vertices[0];
            let row = nw / patch.width;
            let column = nw % patch.width;
            let half = usize::from(triangle.vertices[1] != nw + patch.width);
            triangle.water_code = water[56 + 2 * (row * (patch.width - 1) + column) + half];
        }
        Ok(patch)
    }
    fn with_refined_water(mut self, water: &[u8]) -> Result<Self, TerrainError> {
        if water.len() < 64 {
            return Err(TerrainError::InvalidFormat);
        }
        let u32_at =
            |offset| u32::from_le_bytes(water[offset..offset + 4].try_into().unwrap()) as usize;
        let extra_count = u32_at(56);
        let triangle_count = u32_at(60);
        let index_offset = extra_count
            .checked_mul(24)
            .and_then(|count| count.checked_add(64))
            .ok_or(TerrainError::InvalidFormat)?;
        let expected_size = triangle_count
            .checked_mul(13)
            .and_then(|count| count.checked_add(index_offset));
        if self
            .vertices
            .len()
            .checked_add(extra_count)
            .is_none_or(|count| count > 4_000_000)
            || triangle_count == 0
            || triangle_count > 8_000_000
            || expected_size != Some(water.len())
        {
            return Err(TerrainError::InvalidFormat);
        }
        let [west, south, east, north] = self.bounds();
        for index in 0..extra_count {
            let coordinate = |axis: usize| {
                let offset = 64 + index * 24 + axis * 8;
                f64::from_le_bytes(water[offset..offset + 8].try_into().unwrap())
            };
            let point = Vec3d::new(coordinate(0), coordinate(1), coordinate(2));
            let g = ecef_to_geodetic(point).map_err(|_| TerrainError::InvalidGrid)?;
            if g.height_m > 100_000.0
                || g.lat_deg < south - 1e-6
                || g.lat_deg > north + 1e-6
                || g.lon_deg < west - 1e-6
                || g.lon_deg > east + 1e-6
            {
                return Err(TerrainError::InvalidGrid);
            }
            self.max_height = self.max_height.max(g.height_m);
            self.vertices.push(point);
        }
        self.triangles.clear();
        self.triangles.reserve(triangle_count);
        for index in 0..triangle_count {
            let offset = index_offset + index * 13;
            self.triangles.push(Triangle::checked(
                &self.vertices,
                [u32_at(offset), u32_at(offset + 4), u32_at(offset + 8)],
                water[offset + 12],
            )?);
        }
        self.nodes.clear();
        self.refined = true;
        self.build_node(0, self.triangles.len());
        Ok(self)
    }
    /// Complete canonical triangles, including their categorical water kind.
    /// The iterator follows BVH order; no caller may infer grid order from it.
    pub fn presentation_triangles(
        &self,
    ) -> impl Iterator<Item = ([Vec3d; 3], Option<WaterKind>)> + '_ {
        self.triangles.iter().map(|triangle| {
            (
                triangle.vertices.map(|index| self.vertices[index]),
                WaterKind::from_code(triangle.water_code),
            )
        })
    }
    pub fn has_water(&self) -> bool {
        self.triangles
            .iter()
            .any(|triangle| WaterKind::from_code(triangle.water_code).is_some())
    }
    pub fn requires_canonical_presentation(&self) -> bool {
        self.refined || self.has_water()
    }
    /// Every canonical vertex, including source-native and reconstructed shore points.
    pub fn canonical_vertices(&self) -> &[Vec3d] {
        &self.vertices
    }
    /// Counts in order land, ocean, lake, river, mixed boundary.
    pub fn water_triangle_counts(&self) -> [usize; 5] {
        let mut counts = [0; 5];
        for triangle in &self.triangles {
            counts[usize::from(triangle.water_code)] += 1;
        }
        counts
    }
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    /// Original regular post grid only; refinement vertices are separate.
    pub fn presentation_grid(&self) -> (usize, usize, &[Vec3d]) {
        (
            self.width,
            self.height,
            &self.vertices[..self.width * self.height],
        )
    }
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
    pub fn contains(&self, g: Geodetic) -> bool {
        let [west, south, east, north] = self.bounds();
        g.lon_deg >= west && g.lon_deg <= east && g.lat_deg >= south && g.lat_deg <= north
    }
    fn build_node(&mut self, start: usize, end: usize) -> usize {
        let mut bounds = Bounds::empty();
        for tri in &self.triangles[start..end] {
            for i in tri.vertices {
                bounds.include(self.vertices[i]);
            }
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            left: 0,
            right: 0,
            start,
            count: end - start,
        });
        if end - start > 8 {
            let extent = bounds.max - bounds.min;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            self.triangles[start..end].sort_unstable_by(|a, b| {
                component(a.center, axis).total_cmp(&component(b.center, axis))
            });
            let middle = start + (end - start) / 2;
            let left = self.build_node(start, middle);
            let right = self.build_node(middle, end);
            self.nodes[index] = Node {
                bounds,
                left,
                right,
                start,
                count: 0,
            };
        }
        index
    }
    /// Sheared-coordinate edge functions following Woop/Benthin/Wald's
    /// watertight construction, evaluated entirely in f64, double-sided.
    fn triangle_distance(&self, tri: Triangle, origin: Vec3d, d: Vec3d) -> Option<f64> {
        let abs = Vec3d::new(d.x.abs(), d.y.abs(), d.z.abs());
        let kz = if abs.x >= abs.y && abs.x >= abs.z {
            0
        } else if abs.y >= abs.z {
            1
        } else {
            2
        };
        let mut kx = (kz + 1) % 3;
        let mut ky = (kx + 1) % 3;
        if component(d, kz) < 0.0 {
            std::mem::swap(&mut kx, &mut ky);
        }
        let sx = component(d, kx) / component(d, kz);
        let sy = component(d, ky) / component(d, kz);
        let sz = 1.0 / component(d, kz);
        let points = tri.vertices.map(|i| self.vertices[i] - origin);
        let transformed = points.map(|p| {
            [
                component(p, kx) - sx * component(p, kz),
                component(p, ky) - sy * component(p, kz),
                component(p, kz) * sz,
            ]
        });
        let [a, b, c] = transformed;
        let u = c[0] * b[1] - c[1] * b[0];
        let v = a[0] * c[1] - a[1] * c[0];
        let w = b[0] * a[1] - b[1] * a[0];
        if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
            return None;
        }
        let determinant = u + v + w;
        if determinant == 0.0 {
            return None;
        }
        let distance = (u * a[2] + v * b[2] + w * c[2]) / determinant;
        (distance >= 0.0 && distance.is_finite()).then_some(distance)
    }
    fn result(&self, origin: Vec3d, d: Vec3d, distance: f64, triangle: usize) -> Option<RayHit> {
        let position = origin + d * distance;
        Some(RayHit {
            distance_m: distance,
            position,
            normal: self.triangles[triangle].normal,
            geodetic: ecef_to_geodetic(position).ok()?,
        })
    }
    pub fn intersect(&self, ray: Ray) -> Option<RayHit> {
        self.intersect_classified(ray).map(|(hit, _)| hit)
    }
    fn intersect_classified(&self, ray: Ray) -> Option<(RayHit, u8)> {
        let d = ray.direction.normalized()?;
        if !ray.origin.is_finite() {
            return None;
        }
        let mut closest = f64::INFINITY;
        let mut selected = None;
        let mut stack = [0usize; 64];
        let mut length = 1usize;
        while length > 0 {
            length -= 1;
            let node = self.nodes[stack[length]];
            if node.bounds.entry(ray.origin, d, closest).is_none() {
                continue;
            }
            if node.count > 0 {
                for i in node.start..node.start + node.count {
                    if let Some(distance) = self.triangle_distance(self.triangles[i], ray.origin, d)
                        && distance < closest
                    {
                        closest = distance;
                        selected = Some(i);
                    }
                }
            } else {
                let left = self.nodes[node.left].bounds.entry(ray.origin, d, closest);
                let right = self.nodes[node.right].bounds.entry(ray.origin, d, closest);
                match (left, right) {
                    (Some(a), Some(b)) => {
                        let (near, far) = if a <= b {
                            (node.left, node.right)
                        } else {
                            (node.right, node.left)
                        };
                        stack[length] = far;
                        stack[length + 1] = near;
                        length += 2;
                    }
                    (Some(_), None) => {
                        stack[length] = node.left;
                        length += 1;
                    }
                    (None, Some(_)) => {
                        stack[length] = node.right;
                        length += 1;
                    }
                    _ => {}
                }
            }
        }
        let selected = selected?;
        Some((
            self.result(ray.origin, d, closest, selected)?,
            self.triangles[selected].water_code,
        ))
    }
    /// Slow reference traversal for independent acceleration-structure tests.
    pub fn intersect_brute_force(&self, ray: Ray) -> Option<RayHit> {
        let d = ray.direction.normalized()?;
        if !ray.origin.is_finite() {
            return None;
        }
        let mut closest = f64::INFINITY;
        let mut selected = None;
        for (i, tri) in self.triangles.iter().enumerate() {
            if let Some(distance) = self.triangle_distance(*tri, ray.origin, d)
                && distance < closest
            {
                closest = distance;
                selected = Some(i);
            }
        }
        self.result(ray.origin, d, closest, selected?)
    }
}

#[derive(Clone, Debug, Default)]
pub struct TerrainScene {
    patches: Vec<TerrainPatch>,
    boundary_only: bool,
}
impl TerrainScene {
    pub fn new(patches: Vec<TerrainPatch>) -> Self {
        Self {
            patches,
            boundary_only: false,
        }
    }
    pub fn patches(&self) -> &[TerrainPatch] {
        &self.patches
    }
    /// Whether the canonical Earth occupies a ray, without finding its nearest
    /// surface. A terrain hit already proves occupancy even when a reference
    /// root is closer; rays missing all terrain use the identical footprint
    /// masking and both WGS84 roots as `intersect`.
    pub fn intersects_any(&self, ray: Ray) -> bool {
        if self.boundary_only {
            return intersect_wgs84(ray).is_some();
        }
        if self
            .patches
            .iter()
            .any(|patch| patch.intersect(ray).is_some())
        {
            return true;
        }
        let Some((direction, distances)) = wgs84_distances(ray) else {
            return false;
        };
        distances.into_iter().any(|distance| {
            distance >= 0.0
                && wgs84_hit(ray.origin, direction, distance)
                    .is_some_and(|hit| self.terrain_ground(hit.geodetic).is_none())
        })
    }
    pub fn intersect(&self, ray: Ray) -> Option<SurfaceHit> {
        if self.boundary_only {
            return intersect_wgs84(ray).map(|hit| self.classify_ellipsoid_hit(hit));
        }
        let mut result: Option<SurfaceHit> = None;
        for (index, patch) in self.patches.iter().enumerate() {
            if let Some((hit, code)) = patch.intersect_classified(ray)
                && result.is_none_or(|current| hit.distance_m < current.hit.distance_m)
            {
                result = Some(SurfaceHit {
                    hit,
                    kind: WaterKind::from_code(code).map_or(SurfaceKind::Terrain(index), |kind| {
                        SurfaceKind::Water { patch: index, kind }
                    }),
                    grass_allowed: code == 0,
                });
            }
        }
        if let Some((direction, distances)) = wgs84_distances(ray) {
            for distance in distances {
                // Positive terrain is often nearer than either ellipsoid root.
                // Skip a provably occluded root before any footprint query.
                if distance < 0.0
                    || result.is_some_and(|current| distance >= current.hit.distance_m)
                {
                    continue;
                }
                if let Some(hit) = wgs84_hit(ray.origin, direction, distance)
                    && self.terrain_ground(hit.geodetic).is_none()
                {
                    result = Some(SurfaceHit {
                        hit,
                        kind: SurfaceKind::ReferenceEllipsoid,
                        grass_allowed: true,
                    });
                }
            }
        }
        result
    }
    /// Ground is the same triangle mesh intersected along geodetic up, not a
    /// separate bilinear height model. Supplied height is ignored.
    pub fn ground(&self, location: Geodetic) -> Result<SurfaceHit, TerrainError> {
        let surface = Geodetic::new(location.lat_deg, location.lon_deg, 0.0)?;
        if self.boundary_only {
            return Ok(self.classify_ellipsoid_hit(RayHit {
                distance_m: 0.0,
                position: geodetic_to_ecef(surface),
                normal: EnuBasis::at(surface).up,
                geodetic: surface,
            }));
        }
        if let Some(hit) = self.terrain_ground(surface) {
            return Ok(hit);
        }
        Ok(SurfaceHit {
            hit: RayHit {
                distance_m: 0.0,
                position: geodetic_to_ecef(surface),
                normal: EnuBasis::at(surface).up,
                geodetic: surface,
            },
            kind: SurfaceKind::ReferenceEllipsoid,
            grass_allowed: true,
        })
    }
    // Mesh edges are Cartesian chords, whose geographic footprints are not
    // exactly the rectangular post bounds. Query the actual mesh, including
    // its boundary, rather than masking a rectangle and leaving tiny holes.
    fn terrain_ground(&self, surface: Geodetic) -> Option<SurfaceHit> {
        let mut result: Option<SurfaceHit> = None;
        for (index, patch) in self.patches.iter().enumerate() {
            let origin = geodetic_to_ecef(surface.with_height(patch.max_height + 1000.0).ok()?);
            let ray = Ray {
                origin,
                direction: -EnuBasis::at(surface).up,
            };
            let Some((mut hit, code)) = patch.intersect_classified(ray) else {
                continue;
            };
            if hit.distance_m > patch.max_height + 1000.0 - MIN_HEIGHT_M {
                continue; // Exclude intersections on the opposite side of Earth.
            }
            // A normal-line intersection preserves the requested geographic
            // latitude/longitude exactly; inverse conversion recovers height.
            hit.geodetic.lat_deg = surface.lat_deg;
            hit.geodetic.lon_deg = surface.lon_deg;
            if result.is_none_or(|previous| hit.geodetic.height_m > previous.hit.geodetic.height_m)
            {
                result = Some(SurfaceHit {
                    hit,
                    kind: WaterKind::from_code(code).map_or(SurfaceKind::Terrain(index), |kind| {
                        SurfaceKind::Water { patch: index, kind }
                    }),
                    grass_allowed: code == 0,
                });
            }
        }
        result
    }
    pub fn place_on_ground(
        &self,
        observer: &mut Observer,
        location: Geodetic,
    ) -> Result<(), TerrainError> {
        let hit = self.ground(location)?;
        observer.feet = hit.hit.geodetic;
        observer.pitch_deg = 0.0;
        observer.speed_mps = 10.0;
        Ok(())
    }
    pub fn try_teleport(
        &self,
        observer: &mut Observer,
        location: Geodetic,
    ) -> Result<(), TerrainError> {
        Geodetic::new(location.lat_deg, location.lon_deg, location.height_m)?;
        if location.height_m > observer.maximum_feet_height_m() {
            return Err(GeometryError::ObserverHeight.into());
        }
        let ground = self.ground(location)?;
        if (self.boundary_only && location.height_m < 0.0)
            || location.height_m + 1e-7 < ground.hit.geodetic.height_m
        {
            return Err(TerrainError::BelowSurface);
        }
        observer.feet = location;
        Ok(())
    }
    pub fn fly(
        &self,
        observer: &mut Observer,
        forward: f64,
        right: f64,
        up: f64,
        elapsed_seconds: f64,
        boost: bool,
    ) {
        if ![forward, right, up, elapsed_seconds]
            .iter()
            .all(|v| v.is_finite())
            || elapsed_seconds <= 0.0
        {
            return;
        }
        let frame = observer.frame();
        let enu = EnuBasis::at(observer.feet);
        let Some(direction) = (frame.forward * forward.clamp(-1.0, 1.0)
            + frame.right * right.clamp(-1.0, 1.0)
            + enu.up * up.clamp(-1.0, 1.0))
        .normalized() else {
            return;
        };
        if let Ok(ground) = self.ground(observer.feet)
            && observer.feet.height_m - ground.hit.geodetic.height_m <= 1e-5
            && direction.dot(ground.hit.normal) < -1e-12
        {
            return;
        }
        let distance = observer.speed_mps.clamp(MIN_SPEED_MPS, MAX_SPEED_MPS)
            * elapsed_seconds.min(0.25)
            * if boost { 8.0 } else { 1.0 };
        let origin = geodetic_to_ecef(observer.feet);
        let clearance = 1e-5;
        let ray = Ray {
            origin: origin + direction * clearance,
            direction,
        };
        let actual = self
            .intersect(ray)
            .map_or(distance, |hit| (hit.hit.distance_m).min(distance));
        let Ok(mut location) = inverse_near_surface(origin + direction * actual) else {
            return;
        };
        location.height_m = location.height_m.min(observer.maximum_feet_height_m());
        let Ok(ground) = self.ground(location) else {
            return;
        };
        // Boundaries between an imported patch and the reference fallback can
        // contain a discontinuity; do not teleport through that boundary wall.
        if location.height_m + 1e-6 < ground.hit.geodetic.height_m {
            return;
        }
        if self.boundary_only {
            location.height_m = location.height_m.max(0.0);
        }
        observer.feet = location;
        let _ = observer.set_view_axes(frame.forward, frame.up);
    }
    /// Generic source-derived inspection pose: select the loaded region with
    /// most ocean faces, then the coast nearest its geographic center.
    pub fn boundary_observer(&self) -> Result<Observer, TerrainError> {
        let patch = self
            .patches
            .iter()
            .filter(|p| p.is_boundary_only())
            .max_by_key(|p| p.triangles.iter().filter(|t| t.water_code == 1).count())
            .ok_or(TerrainError::MissingTerrain)?;
        let [west, south, east, north] = patch.bounds();
        let search = geodetic_to_ecef(Geodetic::new(
            (south + north) * 0.5,
            (west + east) * 0.5,
            0.0,
        )?);
        let mut edges = std::collections::BTreeMap::<(usize, usize), (bool, Option<Vec3d>)>::new();
        for triangle in &patch.triangles {
            if triangle.water_code > 1 {
                continue;
            }
            let [a, b, c] = triangle.vertices;
            for (start, end) in [(a, b), (b, c), (c, a)] {
                let entry = edges.entry((start.min(end), start.max(end))).or_default();
                if triangle.water_code == 1 {
                    entry.0 = true;
                } else {
                    entry.1 = Some(triangle.center);
                }
            }
        }
        let coastline: Vec<_> = edges
            .into_iter()
            .filter(|(_, (ocean, land))| *ocean && land.is_some())
            .map(|(edge, (_, land))| (edge, land.unwrap()))
            .collect();
        let &((start, end), land) = coastline
            .iter()
            .min_by(|((a, b), _), ((c, d), _)| {
                ((patch.vertices[*a] + patch.vertices[*b]) / 2.0)
                    .distance(search)
                    .total_cmp(&((patch.vertices[*c] + patch.vertices[*d]) / 2.0).distance(search))
            })
            .ok_or(TerrainError::MissingTerrain)?;
        let mut adjacent = std::collections::BTreeMap::<usize, Vec<usize>>::new();
        for &((a, b), _) in &coastline {
            adjacent.entry(a).or_default().push(b);
            adjacent.entry(b).or_default().push(a);
        }
        // A single native contour segment can follow a raster axis. Trace a
        // broader connected stretch to obtain a reproducible inspection
        // direction, without changing any coastline vertices or heights.
        let follow = |mut current: usize, mut previous: usize| {
            let mut remaining = 600.0;
            let mut visited = std::collections::BTreeSet::new();
            visited.insert(previous);
            while visited.insert(current) {
                let Some(neighbors) = adjacent.get(&current) else {
                    break;
                };
                // A branch is not a unique continuation; stop rather than
                // inventing a preferred shoreline branch.
                let next: Vec<_> = neighbors
                    .iter()
                    .copied()
                    .filter(|&v| v != previous)
                    .collect();
                if next.len() != 1 || visited.contains(&next[0]) {
                    break;
                }
                let next = next[0];
                let from = patch.vertices[current];
                let to = patch.vertices[next];
                let length = from.distance(to);
                if length >= remaining {
                    return from + (to - from) * (remaining / length);
                }
                remaining -= length;
                previous = current;
                current = next;
            }
            patch.vertices[current]
        };
        let location = ecef_to_geodetic(land)?;
        let mut observer = Observer::new(location);
        self.place_on_ground(&mut observer, location)?;
        let elevated = observer.feet.with_height(observer.feet.height_m + 6.0)?;
        self.try_teleport(&mut observer, elevated)?;
        let shore = (patch.vertices[start] + patch.vertices[end]) / 2.0;
        let mut along = (follow(end, start) - follow(start, end))
            .normalized()
            .ok_or(TerrainError::MissingTerrain)?;
        // Choose the direction with the adjacent source land on the right.
        if along
            .cross(EnuBasis::at(observer.feet).up)
            .dot(land - shore)
            < 0.0
        {
            along = -along;
        }
        observer.aim_at(shore + along * 600.0)?;
        observer.pitch_deg = -4.0; // Application viewing choice, not source data.
        observer.speed_mps = 10.0;
        Ok(observer)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn render_pixel(
        &self,
        frame: CameraFrame,
        lens: Lens,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        display_size: [f32; 2],
        global_texture: Option<TextureView<'_>>,
        patch_textures: &[Option<PatchTextureView<'_>>],
    ) -> [u8; 4] {
        let Some(ray) = lens.pixel_ray_for_view(frame, x, y, width, height, display_size) else {
            return [4, 7, 14, 255];
        };
        let Some(surface) = self.intersect(ray) else {
            return [9, 17, 30, 255];
        };
        let hit = surface.hit;
        let local_color = match surface.kind {
            SurfaceKind::Terrain(index) | SurfaceKind::Water { patch: index, .. } => patch_textures
                .get(index)
                .copied()
                .flatten()
                .and_then(|t| t.sample_rgba(hit.geodetic)),
            SurfaceKind::ReferenceEllipsoid => None,
        };
        let fallback = global_texture
            .map(|t| t.sample(hit.geodetic))
            .unwrap_or([76, 116, 94]);
        let color: [u8; 3] = local_color.map_or(fallback, |local| {
            let coverage = f64::from(local[3]) / 255.0;
            std::array::from_fn(|channel| {
                (f64::from(local[channel]) * coverage
                    + f64::from(fallback[channel]) * (1.0 - coverage))
                    .round()
                    .clamp(0.0, 255.0) as u8
            })
        });
        // Oblique camera-relative fill reveals geometric slopes. This is
        // illustrative lighting, not a claim about date/time solar position.
        let light = (frame.up * 0.5 - frame.right * 0.4 - frame.forward * 0.8)
            .normalized()
            .unwrap();
        let shade = 0.45 + 0.55 * hit.normal.dot(light).max(0.0);
        [
            (color[0] as f64 * shade) as u8,
            (color[1] as f64 * shade) as u8,
            (color[2] as f64 * shade) as u8,
            255,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureProjection {
    Geographic,
    WebMercator,
}
/// Full image outer bounds in geographic degrees, including any mosaic margin.
#[derive(Clone, Copy)]
pub struct PatchTextureView<'a> {
    pub texture: TextureView<'a>,
    pub projection: TextureProjection,
    pub west_deg: f64,
    pub south_deg: f64,
    pub east_deg: f64,
    pub north_deg: f64,
}
impl PatchTextureView<'_> {
    pub fn uv(self, g: Geodetic) -> Option<[f64; 2]> {
        let latitude_limit = match self.projection {
            TextureProjection::Geographic => 90.0,
            TextureProjection::WebMercator => PI.sinh().atan().to_degrees(),
        };
        if ![
            self.west_deg,
            self.south_deg,
            self.east_deg,
            self.north_deg,
            g.lat_deg,
            g.lon_deg,
        ]
        .iter()
        .all(|x| x.is_finite())
            || self.south_deg < -latitude_limit
            || self.north_deg > latitude_limit
            || g.lon_deg < self.west_deg
            || g.lon_deg > self.east_deg
            || g.lat_deg < self.south_deg
            || g.lat_deg > self.north_deg
            || self.east_deg <= self.west_deg
            || self.north_deg <= self.south_deg
        {
            return None;
        }
        let u = (g.lon_deg - self.west_deg) / (self.east_deg - self.west_deg);
        let y = |latitude: f64| match self.projection {
            TextureProjection::Geographic => latitude,
            TextureProjection::WebMercator => latitude.to_radians().tan().asinh(),
        };
        let v = (y(self.north_deg) - y(g.lat_deg)) / (y(self.north_deg) - y(self.south_deg));
        Some([u, v])
    }
    pub fn sample(self, g: Geodetic) -> Option<[u8; 3]> {
        let [r, g, b, _] = self.sample_rgba(g)?;
        Some([r, g, b])
    }
    /// Bilinear encoded-color sampling with premultiplied alpha weights.
    /// Returns straight RGB plus interpolated coverage; missing/fully transparent
    /// samples return None. Alpha is imagery coverage, never terrain geometry.
    pub fn sample_rgba(self, g: Geodetic) -> Option<[u8; 4]> {
        let [u, v] = self.uv(g)?;
        let texture = self.texture;
        if texture.width == 0 || texture.height == 0 {
            return None;
        }
        // Pixel centers are half a pixel inside the outer mosaic bounds.
        // Interpolate stored color channels, with edge replication. This is
        // visual filtering only, not added source detail or physical radiometry.
        let x = (u * texture.width as f64 - 0.5).clamp(0.0, texture.width as f64 - 1.0);
        let y = (v * texture.height as f64 - 0.5).clamp(0.0, texture.height as f64 - 1.0);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(texture.width as usize - 1);
        let y1 = (y0 + 1).min(texture.height as usize - 1);
        let tx = x - x0 as f64;
        let ty = y - y0 as f64;
        let pixel = |column, row| {
            let offset = (row * texture.width as usize + column) * 4;
            texture.rgba.get(offset..offset + 4)
        };
        let (a, b, c, d) = (
            pixel(x0, y0)?,
            pixel(x1, y0)?,
            pixel(x0, y1)?,
            pixel(x1, y1)?,
        );
        let pixels = [a, b, c, d];
        let weights = [
            (1.0 - tx) * (1.0 - ty),
            tx * (1.0 - ty),
            (1.0 - tx) * ty,
            tx * ty,
        ];
        let alpha = pixels
            .iter()
            .zip(weights)
            .map(|(pixel, weight)| f64::from(pixel[3]) * weight)
            .sum::<f64>();
        let alpha_byte = alpha.round().clamp(0.0, 255.0) as u8;
        if alpha_byte == 0 {
            return None;
        }
        let rgb: [u8; 3] = std::array::from_fn(|channel| {
            (pixels
                .iter()
                .zip(weights)
                .map(|(pixel, weight)| f64::from(pixel[channel]) * f64::from(pixel[3]) * weight)
                .sum::<f64>()
                / alpha)
                .round()
                .clamp(0.0, 255.0) as u8
        });
        Some([rgb[0], rgb[1], rgb[2], alpha_byte])
    }
}
