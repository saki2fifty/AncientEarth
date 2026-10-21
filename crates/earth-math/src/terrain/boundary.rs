//! Height-free categorical regions on the analytic WGS84 ellipsoid.
use super::*;

#[derive(Clone, Debug)]
pub(super) struct BoundaryData {
    parameters: Vec<[f64; 2]>,
    cells: Vec<Vec<usize>>,
}

fn cross(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

impl TerrainPatch {
    /// Read a categorical asset containing geographic parameters and faces,
    /// never heights. Presentation vertices are evaluated at WGS84 h=0.
    pub fn from_boundary_bytes(bytes: &[u8]) -> Result<Self, TerrainError> {
        if bytes.len() < 56 || &bytes[..8] != b"AEBNDR01" {
            return Err(TerrainError::InvalidFormat);
        }
        let u32_at = |o| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
        let f64_at = |o| f64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        let (width, height, vertices, faces) = (u32_at(8), u32_at(12), u32_at(48), u32_at(52));
        let base_count = width
            .checked_mul(height)
            .ok_or(TerrainError::InvalidFormat)?;
        if width < 2
            || height < 2
            || base_count > 4_000_000
            || vertices < base_count
            || vertices > 4_000_000
            || faces == 0
            || faces > 8_000_000
            || vertices
                .checked_mul(16)
                .and_then(|n| faces.checked_mul(13).and_then(|m| n.checked_add(m)))
                .and_then(|n| n.checked_add(56))
                != Some(bytes.len())
        {
            return Err(TerrainError::InvalidFormat);
        }
        let mut patch = Self::from_grid(
            width,
            height,
            f64_at(16),
            f64_at(24),
            f64_at(32),
            f64_at(40),
            vec![0.0; base_count],
        )?;
        let parameters: Vec<_> = (0..vertices)
            .map(|i| [f64_at(56 + 16 * i), f64_at(64 + 16 * i)])
            .collect();
        for (index, &[row, column]) in parameters.iter().enumerate() {
            if !row.is_finite()
                || !column.is_finite()
                || row < 0.0
                || column < 0.0
                || row > 3.0 * (height - 1) as f64
                || column > 3.0 * (width - 1) as f64
                || (index < base_count
                    && [row, column]
                        != [3.0 * (index / width) as f64, 3.0 * (index % width) as f64])
            {
                return Err(TerrainError::InvalidGrid);
            }
        }
        patch.vertices = parameters
            .iter()
            .map(|&p| patch.parameter_geodetic(p).map(geodetic_to_ecef))
            .collect::<Result<_, _>>()?;
        patch.triangles.clear();
        for index in 0..faces {
            let offset = 56 + 16 * vertices + 13 * index;
            let indices = [u32_at(offset), u32_at(offset + 4), u32_at(offset + 8)];
            if indices.iter().any(|&i| i >= vertices) {
                return Err(TerrainError::InvalidGrid);
            }
            let [a, b, c] = indices.map(|i| parameters[i]);
            if cross(a, b, c) <= 0.0 {
                return Err(TerrainError::InvalidGrid);
            }
            patch.triangles.push(Triangle::checked(
                &patch.vertices,
                indices,
                bytes[offset + 12],
            )?);
        }
        patch.nodes.clear();
        patch.build_node(0, patch.triangles.len());
        // BVH construction reorders faces; build the categorical index afterward.
        let mut cells = vec![Vec::new(); (width - 1) * (height - 1)];
        for (index, triangle) in patch.triangles.iter().enumerate() {
            let points = triangle.vertices.map(|i| parameters[i]);
            let mean = [
                (points[0][0] + points[1][0] + points[2][0]) / 3.0,
                (points[0][1] + points[1][1] + points[2][1]) / 3.0,
            ];
            let row = ((mean[0] / 3.0).floor() as usize).min(height - 2);
            let col = ((mean[1] / 3.0).floor() as usize).min(width - 2);
            // Every face belongs to one registered coarse cell. Reject files
            // whose topology cannot be indexed by that contract.
            if points.iter().any(|p| {
                p[0] < row as f64 * 3.0 - 1e-9
                    || p[0] > (row + 1) as f64 * 3.0 + 1e-9
                    || p[1] < col as f64 * 3.0 - 1e-9
                    || p[1] > (col + 1) as f64 * 3.0 + 1e-9
            }) {
                return Err(TerrainError::InvalidGrid);
            }
            cells[row * (width - 1) + col].push(index);
        }
        if cells.iter().any(Vec::is_empty) {
            return Err(TerrainError::InvalidGrid);
        }
        patch.refined = true;
        patch.boundary = Some(BoundaryData { parameters, cells });
        Ok(patch)
    }

    pub fn is_boundary_only(&self) -> bool {
        self.boundary.is_some()
    }

    /// Local [row,column] in native WBM post units, north-west origin. The
    /// retained base posts are separated by three units. No height component.
    pub fn boundary_parameters(&self) -> Option<&[[f64; 2]]> {
        self.boundary
            .as_ref()
            .map(|data| data.parameters.as_slice())
    }

    /// Indexed parameter-space triangles and source category code (0 land,
    /// 1 ocean, 2 lake, 3 river, 4 unresolved). Indices address the parameters
    /// and canonical display vertices in their identical stored order.
    pub fn boundary_triangles(&self) -> Option<impl Iterator<Item = ([usize; 3], u8)> + '_> {
        self.boundary.as_ref()?;
        Some(
            self.triangles
                .iter()
                .map(|triangle| (triangle.vertices, triangle.water_code)),
        )
    }

    fn parameter_geodetic(&self, [row, column]: [f64; 2]) -> Result<Geodetic, TerrainError> {
        let [west, south, east, north] = self.bounds();
        let lat = if row == 0.0 {
            north
        } else if row == 3.0 * (self.height - 1) as f64 {
            south
        } else {
            north - row * (self.step_lat / 3.0)
        };
        let lon = if column == 0.0 {
            west
        } else if column == 3.0 * (self.width - 1) as f64 {
            east
        } else {
            west + column * (self.step_lon / 3.0)
        };
        Ok(Geodetic::new(lat, lon, 0.0)?)
    }

    /// Exact h=0 geographic parameter used to construct this presentation vertex.
    /// Region extrema use the identical arithmetic as `bounds()`.
    pub fn boundary_geodetic(&self, index: usize) -> Option<Geodetic> {
        self.parameter_geodetic(*self.boundary.as_ref()?.parameters.get(index)?)
            .ok()
    }

    fn boundary_class(&self, location: Geodetic) -> Option<u8> {
        let data = self.boundary.as_ref()?;
        let [west, south, east, north] = self.bounds();
        if location.lon_deg < west
            || location.lon_deg > east
            || location.lat_deg < south
            || location.lat_deg > north
        {
            return None;
        }
        let point = [
            (north - location.lat_deg) / (self.step_lat / 3.0),
            (location.lon_deg - west) / (self.step_lon / 3.0),
        ];
        let row = ((point[0] / 3.0).floor() as usize).min(self.height - 2);
        let col = ((point[1] / 3.0).floor() as usize).min(self.width - 2);
        let mut result = None;
        // Neighbor bins handle roundoff at exact shared coarse-cell borders.
        for r in row.saturating_sub(1)..=(row + 1).min(self.height - 2) {
            for c in col.saturating_sub(1)..=(col + 1).min(self.width - 2) {
                for &index in &data.cells[r * (self.width - 1) + c] {
                    let triangle = &self.triangles[index];
                    let [a, b, c] = triangle.vertices.map(|i| data.parameters[i]);
                    // 1e-8 source-post units is numerical edge handling, not
                    // coastline uncertainty. Prefer non-grass at shared edges.
                    if [(a, b), (b, c), (c, a)].into_iter().all(|(a, b)| {
                        cross(a, b, point) >= -1e-8 * (b[0] - a[0]).hypot(b[1] - a[1])
                    }) {
                        result =
                            Some(result.map_or(triangle.water_code, |old: u8| {
                                old.max(triangle.water_code)
                            }));
                    }
                }
            }
        }
        result
    }
}

impl TerrainScene {
    /// All physical queries use the exact WGS84 ellipsoid. Patches provide only
    /// finite regional categories; unclassified locations do not imply land.
    pub fn boundary_only(patches: Vec<TerrainPatch>) -> Result<Self, TerrainError> {
        if patches.iter().any(|patch| !patch.is_boundary_only()) {
            return Err(TerrainError::InvalidGrid);
        }
        Ok(Self {
            patches,
            boundary_only: true,
        })
    }
    pub fn is_boundary_only(&self) -> bool {
        self.boundary_only
    }

    pub(super) fn classify_ellipsoid_hit(&self, hit: RayHit) -> SurfaceHit {
        for (index, patch) in self.patches.iter().enumerate() {
            if let Some(code) = patch.boundary_class(hit.geodetic) {
                if code == 4 {
                    return SurfaceHit {
                        hit,
                        kind: SurfaceKind::ReferenceEllipsoid,
                        grass_allowed: false,
                    };
                }
                return SurfaceHit {
                    hit,
                    kind: WaterKind::from_code(code).map_or(SurfaceKind::Terrain(index), |kind| {
                        SurfaceKind::Water { patch: index, kind }
                    }),
                    grass_allowed: code == 0,
                };
            }
        }
        SurfaceHit {
            hit,
            kind: SurfaceKind::ReferenceEllipsoid,
            grass_allowed: false,
        }
    }
}
