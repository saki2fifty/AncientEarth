//! Scalar f32 transcription of refraction.wesl's ray geometry. This checks
//! quantized uniforms and arithmetic against independent f64/oracle tests;
//! it does not emulate native GPU contraction or transcendental implementations.
use earth_math::{
    Observer, WGS84_A_M, WGS84_E2,
    atmosphere::{AtmosphereProfile, gpu::GpuEllipsoid},
};
type V = [f32; 3];
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn mul(a: V, s: f32) -> V {
    a.map(|x| x * s)
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: V) -> V {
    mul(a, 1.0 / norm(a))
}
#[derive(Clone, Copy)]
struct State {
    p: V,
    u: V,
}
pub struct Trace {
    pub position: V,
    pub direction: V,
    pub path: f32,
    pub status: u32,
}
pub struct Model {
    geometry: GpuEllipsoid,
    supported: bool,
    layers: Vec<[f32; 3]>,
    curvature: f32,
    top: f32,
}
impl Model {
    pub fn new(observer: Observer, profile: &AtmosphereProfile) -> Self {
        let layers = profile
            .layers()
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let gradient = profile.layers().get(i + 1).map_or(0.0, |n| {
                    (n.refractivity - k.refractivity) / (n.height_m - k.height_m)
                });
                [k.height_m as f32, k.refractivity as f32, gradient as f32]
            })
            .collect();
        Self {
            geometry: GpuEllipsoid::at(observer),
            supported: earth_math::atmosphere::gpu::display_supported(observer),
            layers,
            curvature: (1.0 / (WGS84_A_M * (1.0 - WGS84_E2)) + profile.maximum_index_gradient())
                as f32,
            top: profile.top_height_m() as f32,
        }
    }
    fn geometry(&self, p: V) -> (f32, V) {
        let qp = std::array::from_fn(|i| {
            self.geometry.origin[i] + dot(self.geometry.rows[i][..3].try_into().unwrap(), p)
        });
        if norm(qp) < 0.75 {
            (-1000000.0, [0.0, 1.0, 0.0])
        } else {
            self.geometry.height_normal(p)
        }
    }
    fn bracket(&self, h: f32) -> usize {
        let mut low = 0;
        let mut high = self.layers.len() - 1;
        for _ in 0..8 {
            if high <= low + 1 {
                break;
            }
            let mid = (low + high) / 2;
            if self.layers[mid][0] <= h {
                low = mid;
            } else {
                high = mid;
            }
        }
        low
    }
    fn index(&self, h: f32) -> [f32; 2] {
        if h >= self.top {
            return [0.0, 0.0];
        }
        let k = self.layers[self.bracket(h)];
        [k[1] + k[2] * (h - k[0]), k[2]]
    }
    fn knot_bound(&self, gap: f32, velocity: f32) -> f32 {
        let root = (velocity * velocity + 2.0 * self.curvature * gap).sqrt();
        if velocity >= 0.0 {
            2.0 * gap / (root + velocity).max(1e-20)
        } else {
            (root - velocity) / self.curvature
        }
    }
    fn layer_step(&self, h: f32, v: f32, band: f32) -> f32 {
        let low = self.bracket(h);
        let last = (low + 1).min(self.layers.len() - 2);
        for i in low.saturating_sub(1)..=last {
            if self.layers[i + 1][0] - self.layers[i][0] < 4.0 * band {
                return -1.0;
            }
        }
        let mut limit = 1e30_f32;
        let below = if h - self.layers[low][0] <= band {
            low.checked_sub(1)
        } else {
            Some(low)
        };
        if let Some(i) = below {
            let gap = h - self.layers[i][0];
            if gap > band {
                limit = limit.min(self.knot_bound(gap, -v));
            }
        }
        let mut above = low + 1;
        if self.layers[above][0] - h <= band {
            above += 1;
        }
        if above < self.layers.len() {
            let gap = self.layers[above][0] - h;
            if gap > band {
                limit = limit.min(self.knot_bound(gap, v));
            }
        }
        limit
    }
    fn midpoint(&self, s: State, step: f32) -> State {
        let (height, up) = self.geometry(s.p);
        let [n, g] = self.index(height);
        let bend = mul(sub(up, mul(s.u, dot(s.u, up))), g / (1.0 + n));
        let mp = add(s.p, mul(s.u, step * 0.5));
        let mu = unit(add(s.u, mul(bend, step * 0.5)));
        let (height, up) = self.geometry(mp);
        let [n, g] = self.index(height);
        State {
            p: add(s.p, mul(mu, step)),
            u: unit(add(
                s.u,
                mul(sub(up, mul(mu, dot(mu, up))), g / (1.0 + n) * step),
            )),
        }
    }
    fn advance(&self, s: State, step: f32) -> State {
        self.midpoint(self.midpoint(s, step * 0.5), step * 0.5)
    }
    pub fn trace(&self, direction: V) -> Trace {
        let mut s = State {
            p: [0.0; 3],
            u: direction,
        };
        let mut path = 0.0;
        let mut proposed = 2000.0_f32;
        let finish = |s: State, path, status| Trace {
            position: s.p,
            direction: s.u,
            path,
            status,
        };
        let eye = self.geometry.center[3];
        if !self.supported {
            return finish(s, path, 2);
        }
        if eye > self.top {
            let o = self.geometry.center[..3].try_into().unwrap();
            let o = mul(o, -1.0);
            let along = dot(o, direction);
            let distance = norm(cross(o, direction));
            let radius = 6378137.0 + self.top;
            if distance > radius || along >= 0.0 {
                return finish(s, path, 0);
            }
            let span = ((radius - distance) * (radius + distance)).max(0.0).sqrt();
            let lower = (-along - span).max(0.0);
            let mut a = lower;
            let mut b = -along + span;
            for _ in 0..40 {
                let x = a + (b - a) * 0.38196602;
                let y = a + (b - a) * 0.618034;
                if self.geometry(mul(direction, x)).0 < self.geometry(mul(direction, y)).0 {
                    b = y;
                } else {
                    a = x;
                }
            }
            let mut upper = (a + b) * 0.5;
            if self.geometry(mul(direction, upper)).0 > self.top {
                return finish(s, path, 0);
            }
            let mut low = lower;
            for _ in 0..32 {
                let mid = (low + upper) * 0.5;
                if self.geometry(mul(direction, mid)).0 > self.top {
                    low = mid;
                } else {
                    upper = mid;
                }
            }
            s.p = mul(direction, upper);
            path = upper;
        }
        let entry = path;
        for _ in 0..4096 {
            let (h, up) = self.geometry(s.p);
            let vertical = dot(s.u, up);
            if h >= self.top - 0.01 && vertical > 0.0 {
                return finish(s, path, 0);
            }
            if path - entry > 2000000.0 {
                return finish(s, path, 2);
            }
            let disc = (vertical * vertical + 2.0 * self.curvature * h.max(0.0))
                .max(0.0)
                .sqrt();
            let safe = if vertical < 0.0 {
                2.0 * h.max(0.0) / (disc - vertical).max(1e-20)
            } else {
                (vertical + disc) / self.curvature
            };
            let mut step = proposed.min(2000.0).min((safe * 0.8).max(0.01));
            let tolerance = (4e-7 * norm(s.p)).max(0.002);
            if h < tolerance && vertical < -1e-8 {
                step = (2.0 * h.max(1e-8) / -vertical).clamp(0.01, 1.0);
            }
            let knot = self.layer_step(h, vertical, (4e-7 * norm(s.p)).max(1e-5));
            if knot < 0.0 {
                return finish(s, path, 2);
            }
            step = step.min(knot.max(0.01));
            let full = self.midpoint(s, step);
            let half = self.advance(s, step);
            let error = norm(sub(full.p, half.p)).max(norm(sub(full.u, half.u)) * step) / 3.0;
            if error > tolerance && step > 0.01000001 {
                proposed = step * 0.5;
                continue;
            }
            if error > tolerance {
                return finish(s, path, 2);
            }
            let next = self.geometry(half.p).0;
            if next <= 0.0 || (h < self.top && next >= self.top) {
                let ground = next <= 0.0;
                let mut low = 0.0;
                let mut high = step;
                for _ in 0..20 {
                    let mid = (low + high) * 0.5;
                    let height = self.geometry(self.advance(s, mid).p).0;
                    if if ground {
                        height > 0.0
                    } else {
                        height < self.top
                    } {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                let delta = (low + high) * 0.5;
                return finish(self.advance(s, delta), path + delta, u32::from(ground));
            }
            s = half;
            path += step;
            proposed = if error < tolerance * 0.125 {
                step * 2.0
            } else {
                step
            };
        }
        finish(s, path, 2)
    }
}
