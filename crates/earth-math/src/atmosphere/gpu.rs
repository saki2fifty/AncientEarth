//! Observer-relative f32 presentation coefficients for the f64 optical model.
//! Numerical presentation has finite precision; measurements use the CPU solver.
use crate::{Observer, Vec3d, WGS84_A_M, WGS84_B_M, geodetic_to_ecef, rendering::LocalFrame};

/// Operational eye-height limit for the current observer-relative optical pass.
/// This is a presentation policy, not the physical extent of the atmosphere.
pub const MAX_GPU_EYE_HEIGHT_M: f64 = 20_000.0;

/// Current f32 optical display domain. The application uses geometric rendering
/// AND geometric measurements outside it, retaining the downloaded profile.
/// Within this domain thin layers, turning rays or exhausted path/step budgets
/// can still be unresolved; this does not guarantee convergence of every ray.
pub fn display_supported(observer: Observer) -> bool {
    let height = observer.eye_geodetic().height_m;
    height.is_finite() && (0.0..=MAX_GPU_EYE_HEIGHT_M).contains(&height)
}

#[derive(Clone, Copy, Debug)]
pub struct GpuEllipsoid {
    /// Local rows of a*Q, where Q=diag(1/a�,1/a�,1/b�).
    pub rows: [[f32; 4]; 3],
    /// Local a*Q*O and the stable implicit value a*(O.Q.O-1).
    pub origin: [f32; 4],
    /// Earth center in the eye frame, and geodetic eye height.
    pub center: [f32; 4],
    /// sin/cos of eye geodetic latitude (for relative geographic lookup).
    pub latitude: [f32; 4],
}
impl GpuEllipsoid {
    pub fn at(observer: Observer) -> Self {
        let local = LocalFrame::at(observer);
        let geographic = observer.eye_geodetic();
        let q = |v: Vec3d| {
            Vec3d::new(
                v.x / WGS84_A_M,
                v.y / WGS84_A_M,
                v.z * WGS84_A_M / (WGS84_B_M * WGS84_B_M),
            )
        };
        let axes = [local.basis.east, local.basis.up, -local.basis.north];
        let rows = axes.map(|axis| {
            let values = axes.map(|other| axis.dot(q(other)) as f32);
            [values[0], values[1], values[2], 0.0]
        });
        let mut surface = geographic;
        surface.height_m = 0.0;
        let base = geodetic_to_ecef(surface);
        let height = geographic.height_m;
        let f0 = 2.0 * height * local.basis.up.dot(q(base))
            + height * height * local.basis.up.dot(q(local.basis.up));
        let g = local.direction(q(local.origin));
        let center = local.position(Vec3d::ZERO);
        let (sin, cos) = geographic.lat_deg.to_radians().sin_cos();
        Self {
            rows,
            origin: [g[0], g[1], g[2], f0 as f32],
            center: [center[0], center[1], center[2], height as f32],
            latitude: [sin as f32, cos as f32, 0.0, 0.0],
        }
    }
    /// Scalar f32 counterpart of the shader's local height/normal solver, for
    /// independent numerical comparison with f64 geodetic inversion.
    pub fn height_normal(&self, position: [f32; 3]) -> (f32, [f32; 3]) {
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let mul = |v: [f32; 3]| self.rows.map(|r| dot([r[0], r[1], r[2]], v));
        let normalize = |v: [f32; 3]| {
            let norm = dot(v, v).sqrt();
            v.map(|x| x / norm)
        };
        let mp = mul(position);
        let g = [self.origin[0], self.origin[1], self.origin[2]];
        let qp = std::array::from_fn(|i| g[i] + mp[i]);
        let f = self.origin[3] + dot(position, std::array::from_fn(|i| 2.0 * g[i] + mp[i]));
        let mut normal = normalize(qp);
        let mut height = 0.0;
        for _ in 0..5 {
            let qn = mul(normal);
            let projection = dot(normal, qp);
            let root = (projection * projection - f * dot(normal, qn))
                .max(0.0)
                .sqrt();
            height = f / (projection + root);
            normal = normalize(std::array::from_fn(|i| qp[i] - height * qn[i]));
        }
        (height, normal)
    }
    /// Relative geodetic angles from an analytic outward normal, avoiding
    /// subtraction of nearly equal absolute latitude/longitude in f32.
    pub fn relative_angles(&self, normal: [f32; 3]) -> [f32; 2] {
        let [x, y, z] = normal;
        let [sin, cos, _, _] = self.latitude;
        let a = y * cos + z * sin;
        let h = (a * a + x * x).sqrt();
        let d = if a >= 0.0 && h + a > 0.0 {
            x * x / (h + a)
        } else {
            h - a
        };
        [(-z - d * sin).atan2(y + d * cos), x.atan2(a)]
    }
}
