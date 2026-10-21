//! Scalar f32 execution of the reviewed WESL expressions, compared against
//! independent f64 ephemeris/geometry in the caller. GPU fusion/transcendentals
//! may differ; this is a numerical representation check, not native pixel QA.
use earth_math::astro::GpuSolarEclipse;

#[derive(Clone, Copy)]
struct V([f32; 3]);
impl V {
    fn from4(v: [f32; 4]) -> Self {
        Self([v[0], v[1], v[2]])
    }
    fn add(self, rhs: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] + rhs.0[i]))
    }
    fn sub(self, rhs: Self) -> Self {
        self.add(rhs.scale(-1.0))
    }
    fn scale(self, x: f32) -> Self {
        Self(self.0.map(|v| v * x))
    }
    fn dot(self, rhs: Self) -> f32 {
        self.0[0] * rhs.0[0] + self.0[1] * rhs.0[1] + self.0[2] * rhs.0[2]
    }
    fn cross(self, rhs: Self) -> Self {
        let [a, b, c] = self.0;
        let [d, e, f] = rhs.0;
        Self([b * f - c * e, c * d - a * f, a * e - b * d])
    }
    fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
    fn normalized(self) -> Self {
        self.scale(1.0 / self.length())
    }
}

pub fn visible_disk(d: f32, r: f32) -> f32 {
    if d >= 1.0 + r {
        return 1.0;
    }
    if d <= (1.0 - r).abs() {
        return ((1.0 - r) * (1.0 + r)).max(0.0);
    }
    let k = 1.0 - r;
    let sum = 1.0 + r;
    let chord = 0.5 * (d + k * sum / d);
    let h = ((sum - d) * (d - k) * (d + k) * (d + sum)).max(0.0).sqrt() / (2.0 * d);
    let area = h.atan2(chord) + r * r * h.atan2(d - chord) - d * h;
    (1.0 - area / std::f32::consts::PI).clamp(0.0, 1.0)
}

fn relative(initial: V, velocity: V, offset: V) -> V {
    let distance = initial.length();
    let mut delta = 0.0;
    for _ in 0..6 {
        let shift = offset.scale(-1.0).sub(velocity.scale(delta));
        delta = shift.dot(initial.scale(2.0).add(shift))
            / (initial.add(shift).length() + distance)
            / 299_792_458.0;
    }
    initial.sub(offset).sub(velocity.scale(delta))
}

fn aberration(vector: V, beta: V) -> V {
    let direction = vector.normalized();
    let inv_gamma = (1.0 - beta.dot(beta)).max(0.0).sqrt();
    let p = direction.dot(beta);
    direction
        .scale(inv_gamma)
        .add(beta.scale(1.0 + p / (1.0 + inv_gamma)))
        .scale(1.0 / (1.0 + p))
        .normalized()
}

pub fn visibility(field: GpuSolarEclipse, point: [f32; 3]) -> f32 {
    let offset = V(field
        .local_to_shadow
        .map(|row| V::from4(row).dot(V(point)) + row[3]));
    let sun = relative(V::from4(field.sun), V::from4(field.sun_velocity), offset);
    let moon = relative(V::from4(field.moon), V::from4(field.moon_velocity), offset);
    let ds = sun.length();
    let dm = moon.length();
    if dm >= ds || ds <= field.sun[3] || dm <= field.moon[3] {
        return 1.0;
    }
    let beta = V::from4(field.observer_beta).add(V::from4(field.spin_over_c).cross(offset));
    let us = aberration(sun, beta);
    let um = aberration(moon, beta);
    let alpha = (field.sun[3] / ds).clamp(0.0, 1.0).asin();
    let beta_angle = (field.moon[3] / dm).clamp(0.0, 1.0).asin();
    let separation = us.cross(um).length().atan2(us.dot(um));
    visible_disk(separation / alpha, beta_angle / alpha)
}
