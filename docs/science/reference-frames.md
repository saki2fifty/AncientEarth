# Following Earth and holding a nonrotating view

**Current smooth-surface update:** runtime collision uses only WGS84 h=0. Regional category/display vertices are excluded from physical clearance calculations; the conservative all-rotation bound below therefore uses radius a. References to terrain vertices describe the retained historical terrain branch, not current assets.

**F9 changes the observer's reference-frame constraint.** With following enabled, the feet and camera axes stay fixed in Earth-fixed coordinates while astronomical time advances. With following disabled, the eye and full camera orientation stay fixed in **Earth-centered, ICRF-oriented, nonrotating axes**. Earth can turn underneath that view. F9 again retains the currently displayed position and orientation and makes them Earth-fixed.

The origin follows Earth's center along the DE440 trajectory. The observer does not remain at a fixed solar-system barycentric location while Earth moves away. This is a kinematic navigation constraint, not a gravity, orbital, free-fall or inertial-force simulation. Because its origin accelerates with Earth, “nonrotating” is more precise than calling the entire translated frame globally inertial. It is also not a complete relativistic GCRS space-time transformation. [NAIF's reference-frame documentation](https://naif.jpl.nasa.gov/pub/naif/toolkit_docs/C/req/frames.html) distinguishes frame orientation from frame center and explains the ICRF/J2000 convention used by modern ephemerides.

## Rotation and full pose

Let `C(t)` be the existing `icrf_to_ecef` matrix at the continuous TT instant `t`. It includes the same precession, nutation, Earth rotation and polar motion as the astronomical scene. Its inverse is its transpose because it is a proper orthonormal rotation. The terrestrial/celestial decomposition follows [IERS Conventions Chapter 5](https://iers-conventions.obspm.fr/content/chapter5/icc5.pdf); the actual approximations, EOP coverage and historical/future limits are those in [astronomy.md](astronomy.md). F9 introduces no second constant-rate Earth-rotation model.

At the displayed unlocking instant `t0`, capture the eye `P`, forward `F`, and camera up `U`:

```text
P_hold = transpose(C(t0)) P_ecef(t0)
F_hold = transpose(C(t0)) F_ecef(t0)
U_hold = transpose(C(t0)) U_ecef(t0)

P_ecef(t) = C(t) P_hold
F_ecef(t) = C(t) F_hold
U_ecef(t) = C(t) U_hold
```

The held vectors remain unchanged between navigation edits. Each frame is evaluated from that anchor, rather than repeatedly rotating the previous frame, so there is no accumulated incremental-rotation drift. Ordinary floating-point transformation and coordinate-conversion errors remain. Reversing time evaluates the earlier matrix directly; a paused clock does not rotate the view.

The eye is the held position. Its new geodetic coordinates determine the body's local vertical and feet:

```text
(latitude, longitude, h_eye) = inverse_WGS84(P_ecef)
h_feet = h_eye − configured_effective_eye_offset
```

The view need not stay level with this changing local vertical. With zero-roll right/up axes `R0,U0`, positive roll `r` is right-handed about forward:

```text
R = R0 cos(r) − U0 sin(r)
U = U0 cos(r) + R0 sin(r)
```

`Observer::set_view_axes` normalizes forward, constructs `R=normalize(F×U_supplied)` and `U=R×F`, and recovers heading, pitch and roll in the new ENU frame. The roll is `atan2(U·R0,U·U0)`. Heading is retained for a numerically vertical sight direction; roll absorbs this otherwise arbitrary Euler-angle coordinate choice. The complete physical camera triad remains the authority. Flight now preserves both ECEF forward and ECEF up when the local ENU basis changes.

## Navigation and time changes

The application's order is user interface, navigation, astronomical transport, then rendering. A mouse-look, flight, crouch or ordinary coordinate/ground navigation edit is interpreted at the **last displayed** epoch, even when a calendar edit has already requested a different time. The tracker captures the edited pose with that old matrix, then transports it to the requested new epoch. Crouching first moves the eyes along local vertical while preserving feet, then establishes that new eye anchor. Standing headroom and supported coordinate bounds remain enforced.

A combined eclipse date/location preset explicitly commits a new pose and time together and reanchors with the new matrix. Startup command-line choices do likewise. This avoids accidentally transporting a newly loaded location from an unrelated old date. Relocking preserves the current ECEF position and full triad; it does not force roll to zero or snap the view to a geographic heading.

## Surface checks and large jumps

An unlocked observer can encounter terrain as Earth turns. Each proposed pose is checked against the same canonical terrain/reference surface as navigation. Feet more than **0.000001 m** below that surface are rejected. This is a floating-point acceptance tolerance, not physical terrain accuracy. A failed update leaves both the pose and tracker unchanged; the application restores the accepted instant, pauses time and relocks, with an explanatory status. It does not silently raise the observer onto the new surface.

Updates with absolute simulation-time changes of at most **1 second** use endpoint checks. They **do not certify continuous swept collision** between the two orientations. A thin ridge could pass between those endpoints. One second is an implementation guard, not a derived physical accuracy threshold.

Larger forward or reverse jumps require a conservative, orientation-independent clearance proof. Let:

```text
R_bound = max(WGS84 semi-major axis, norm(each canonical terrain vertex))
safe_for_arbitrary_rotation = |P_hold| − eye_offset > R_bound + 0.000001 m
```

The ellipsoid lies in the radius-`a` ball. Every terrain triangle lies in the ball containing its vertices because a point on a triangle is a convex combination of those vertices and the Euclidean norm is convex. By the triangle inequality, the feet radius is at least eye radius minus eye offset. Thus the displayed inequality puts the feet outside every possible rotated surface represented by the scene. It is sufficient, not necessary: it can reject safe low-altitude time jumps, especially away from the equator. Flying higher or using smaller time steps permits continued inspection. The bound says nothing about real terrain outside the imported model, atmospheric obstacles or an extended human-body volume.

## Apparent astronomy and velocity

Unlocked eye positions still supply topocentric parallax and light-time reception coordinates. Their held geocentric velocity is zero, so their aberration velocity is Earth's barycentric translational velocity. Locked eyes additionally use the existing terrestrial spin approximation. Artificial navigation speeds are not physical relativistic boosts.

The Sun/Moon apparent spheres and observer eclipse readout use the selected eye-motion convention. The separate eclipse field for ground and grass retains co-rotating surface velocities, since those receivers rotate with Earth even when the camera does not. Switching F9 changes the assumed observer velocity instantly; a small aberrational change of celestial apparent direction is therefore possible despite continuous camera pose. This is a constraint switch, not a simulated acceleration maneuver.

Implementation: [`reference_frames.rs`](../../crates/earth-math/src/astro/reference_frames.rs), [`lib.rs`](../../crates/earth-math/src/lib.rs), and [`bodies.rs`](../../crates/earth-math/src/astro/bodies.rs). Independent regression cases are recorded in [verification.md](verification.md).
