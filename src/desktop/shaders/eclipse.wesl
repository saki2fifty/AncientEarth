// Original shared solar-disk visibility model; embedded into both surface
// shaders from one source. This is uniform-radiance angular disk coverage,
// not a limb-darkening, diffraction or atmospheric-radiative-transfer model.

struct EclipseSettings {
    row_x: vec4<f32>,
    row_y: vec4<f32>,
    row_z: vec4<f32>,
    sun: vec4<f32>,
    moon: vec4<f32>,
    sun_velocity: vec4<f32>,
    moon_velocity: vec4<f32>,
    observer_beta: vec4<f32>,
    spin_over_c: vec4<f32>,
};

// Each body starts at its independently retarded anchor-observer epoch.
// Linear target motion corrects the differing receive-time light paths.
fn eclipse_retarded_vector(initial: vec3<f32>, velocity: vec3<f32>, offset: vec3<f32>) -> vec3<f32> {
    let anchor_distance = length(initial);
    var delta = 0.0;
    for (var i=0; i<6; i+=1) {
        let shift = -offset-velocity*delta;
        // Difference of lengths without subtracting two astronomical distances.
        delta = dot(shift,2.0*initial+shift)
            / (length(initial+shift)+anchor_distance) / 299792458.0;
    }
    return initial-offset-velocity*delta;
}

fn eclipse_aberrated_direction(vector: vec3<f32>, beta: vec3<f32>) -> vec3<f32> {
    let direction = normalize(vector);
    let inverse_gamma = sqrt(max(0.0,1.0-dot(beta,beta)));
    let projection = dot(direction,beta);
    return normalize((direction*inverse_gamma
        + beta*(1.0+projection/(1.0+inverse_gamma)))/(1.0+projection));
}

// Both circle dimensions are normalized by the solar angular radius.
fn eclipse_visible_disk_fraction(separation: f32, moon_radius: f32) -> f32 {
    let d = max(separation,0.0);
    let r = max(moon_radius,0.0);
    if d >= 1.0 + r {
        return 1.0;
    }
    if d <= abs(1.0 - r) {
        return max(0.0,(1.0-r)*(1.0+r));
    }
    // The containment branch removes the central zero denominator.
    // Factoring 1-r² and using chord sectors avoids ill-conditioned acos near
    // either contact, including almost equal apparent Sun and Moon radii.
    let radius_difference = 1.0-r;
    let radius_sum = 1.0+r;
    let chord = 0.5*(d+radius_difference*radius_sum/d);
    let radicand = (radius_sum-d)*(d-radius_difference)
        *(d+radius_difference)*(d+radius_sum);
    let height = sqrt(max(radicand,0.0))/(2.0*d);
    let solar_angle = atan2(height,chord);
    let lunar_angle = atan2(height,d-chord);
    let area = solar_angle + r*r*lunar_angle - d*height;
    return clamp(1.0-area/3.141592653589793,0.0,1.0);
}

// Artistic diffuse-sky response, not the measured direct solar fraction.
// A finite residual represents unmodeled light scattered from outside the
// shadow and keeps totality readable without restoring direct sunlight.
fn eclipse_sky_response(visible: f32) -> f32 {
    return 0.04 + 0.96*sqrt(clamp(visible,0.0,1.0));
}

fn eclipse_solar_visibility(position: vec3<f32>, field: EclipseSettings) -> f32 {
    // A zero-radius uniform disables occultation when astronomy is unavailable.
    if field.sun.w <= 0.0 || field.moon.w <= 0.0 {
        return 1.0;
    }
    let point = vec4(position,1.0);
    let offset = vec3(dot(field.row_x,point),dot(field.row_y,point),dot(field.row_z,point));
    let sun = eclipse_retarded_vector(field.sun.xyz,field.sun_velocity.xyz,offset);
    let moon = eclipse_retarded_vector(field.moon.xyz,field.moon_velocity.xyz,offset);
    let sun_distance = length(sun);
    let moon_distance = length(moon);
    if moon_distance >= sun_distance || sun_distance <= field.sun.w || moon_distance <= field.moon.w {
        return 1.0;
    }
    let beta = field.observer_beta.xyz + cross(field.spin_over_c.xyz,offset);
    let sun_direction = eclipse_aberrated_direction(sun,beta);
    let moon_direction = eclipse_aberrated_direction(moon,beta);
    let solar_radius = asin(clamp(field.sun.w/sun_distance,0.0,1.0));
    let lunar_radius = asin(clamp(field.moon.w/moon_distance,0.0,1.0));
    // acos(dot) loses significant digits for almost coincident directions.
    let separation = atan2(length(cross(sun_direction,moon_direction)),
        dot(sun_direction,moon_direction));
    return eclipse_visible_disk_fraction(separation/solar_radius,lunar_radius/solar_radius);
}
