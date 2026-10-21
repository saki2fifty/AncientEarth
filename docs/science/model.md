# Scientific model and conventions

AncientEarth uses the **smooth analytic WGS84 ellipsoid everywhere**, with land and every water category at ellipsoidal height h=0. Regional Copernicus Water Body Masks supply only categorical boundaries. Their height-free importer reads no DSM or geoid inputs. The same analytic f64 surface governs Earth range, ground placement, collision and geometric horizon; finite GPU triangles and materials provide presentation. [water.md](water.md) documents categorical reconstruction and its finite source limits.

No city catalog, pins, named-place runtime presets, photographic imagery or topographic height files are active. Historical elevation/imagery code and fixtures remain archived with their notices. The optional EGM2008 readout is a separate datum comparison and never displaces the surface.

## What the coordinates mean

The model uses the defining WGS84 semi-major axis `a = 6,378,137 m` and inverse flattening `1/f = 298.257223563`. The derived semi-minor axis is `b = a(1-f)` and eccentricity squared is `e² = f(2-f)`. These parameters follow the [NGA WGS84 specification](https://earth-info.nga.mil/index.php?action=wgs84&dir=wgs84).

Latitude is **geodetic** latitude: the ellipsoid normal's angle above the equatorial plane, not the geocentric radius's angle. Longitude is positive east. ECEF coordinates are metres, with X through latitude 0°, longitude 0°; Y through latitude 0°, longitude 90°E; and Z through the north pole.

Heights are measured along the ellipsoid normal. They are **not elevation above mean sea level**, height above terrain, or local building height. The coordinate input records the observer's feet height. Eye height is added separately. Ground navigation places the feet at h=0 along that normal. The general conversion library supports negative heights for datum calculations and archived fixtures, but active navigation rejects feet below zero.

The separate vertical HUD also reports the global EGM2008 geoid beneath the current latitude/longitude, `H = h − N` heights above that reference, and eye/feet clearances above the active ground. These clearances are along geodetic up, not shortest distances to slopes. Every native 2.5-arc-minute geoid sample is preserved; bilinear interpolation does not remove source uncertainty. The geoid is a modeled sea-level reference, not the instantaneous ocean surface. See [geoid.md](geoid.md) for the data, registration, height equations and limitations.

Exact-coordinate navigation rejects feet heights below the active surface or so high that the eyes would exceed the supported height ceiling. Rejected input leaves the current position unchanged. It does not silently adjust the requested destination. The separate ground action explicitly snaps to the surface. Toggling boundary categories cannot change the physical surface. Flight and overview have explicit surface and ceiling constraints.

The conversion library's supported height interval is −10,000 m to +1,000,000,000 m. Deep-Earth Cartesian positions are intentionally rejected rather than choosing among ambiguous geodetic solutions. At a pole, longitude and heading describe a conventional orientation; the position itself does not determine a unique longitude.

## Geodetic conversion

For latitude φ, longitude λ, and height h:

```text
N = a / sqrt(1 − e² sin²φ)
X = (N + h) cosφ cosλ
Y = (N + h) cosφ sinλ
Z = (N(1 − e²) + h) sinφ
```

The inverse starts from `atan2(Z, hypot(X,Y)(1-e²))` and iterates the fixed-point relation `φ = atan2(Z + e²N sinφ, hypot(X,Y))`. This is used only in the documented exterior/near-surface domain. Height comes from projecting onto the ellipsoid normal:

```text
h = hypot(X,Y) cosφ + Z sinφ − a sqrt(1 − e² sin²φ)
```

This expression avoids division by small sine or cosine values at the equator or poles. Published [GeographicLib CartConvert examples](https://geographiclib.sourceforge.io/html/CartConvert.1.html) and independent EPSG fixtures check absolute results; roundtrips supplement them rather than serving as the sole evidence.

## Eye position and local orientation

The person's stature is exactly six **international** feet, or 1.8288 m. The default standing eye height is **1.72 m**, with a **1.00 m** crouching eye height, both above the feet and both explicit configurable application assumptions. Stature alone cannot determine a particular person's anatomical eye height. The conversion uses the international foot, exactly 0.3048 m, as described by [NIST](https://www.nist.gov/pml/us-surveyfoot).

Crouching uses the smaller configured eye offset and preserves the feet position. The resulting eye altitude changes the camera origin, dot range and skyline query. Standing headroom remains reserved at the supported altitude ceiling while crouched; rejected posture changes do not move the feet.

East, north and up are the local geodetic ENU basis from [ESA Navipedia](https://gssc.esa.int/navipedia/index.php/Transformations_between_ECEF_and_ENU_coordinates):

```text
east  = (−sinλ, cosλ, 0)
north = (−sinφ cosλ, −sinφ sinλ, cosφ)
up    = ( cosφ cosλ,  cosφ sinλ, sinφ)
```

Heading increases clockwise from north. Pitch is positive above the local horizon. Before roll, forward follows heading and pitch, right is horizontal, and camera up is `right × forward`. Positive roll is right-handed about forward and rotates both camera right and up. The complete triad is preserved by the nonrotating observer mode; see [reference-frames.md](reference-frames.md).

Flight combines forward/back, right/left, and local up/down, then normalizes the resulting direction so diagonal input does not increase speed. Movement is a straight ECEF displacement, not a geodesic, orbit, or aerodynamics simulation. After moving, the old ECEF forward and camera-up axes are re-expressed in the new local frame, including roll: the camera keeps its world orientation and forward flight continues through the poles. Collision sweeps the feet against the analytic ellipsoid and validates the endpoint. A frame uses at most 0.25 seconds of movement time to prevent a long UI pause from producing a large unintended step. Shift multiplies speed by eight.

## Range at the dot

In geometric mode, the measurement is a separate ray from the **eye** along camera forward. It does not read a depth buffer or infer distance from the displayed raster. The value is Euclidean line-of-sight distance in metres to the nearest eligible nonnegative intersection with the analytic WGS84 ellipsoid. It is not surface distance, geodesic distance, or travel distance. GPU display geometry samples that surface with finite tessellation differences; it does not replace this measurement query.

The following equations describe the active analytic surface. Archived triangle algorithms in [terrain.md](terrain.md) are not used for runtime Earth measurements:

For a ray `P + tD`, dividing coordinates by `(a,a,b)` transforms the ellipsoid into a unit sphere. The direction is normalized after this transform. With scaled origin `O` and scaled unit direction `U`:

```text
B = O · U
C = (|O| − 1)(|O| + 1)
discriminant = 1 − |O × U|²
q = −B − copysign(sqrt(discriminant), B)
roots = q, C/q
```

Each forward root is converted back to Euclidean metres; the nearest eligible one is selected. The factored expression for C reduces cancellation near the surface, and the cross-product discriminant avoids subtracting two large near-equal terms for a distant origin. Nonunit input directions do not change the reported distance.

A negative discriminant within the documented floating-point roundoff threshold is treated as a tangent. Grazing incidence is inherently ill-conditioned: a tiny angular or position perturbation can turn a hit into a miss or change range considerably. The software therefore makes no unconditional claim of a fixed absolute range error.

The dot additionally tests visible grass blades using the current shared wind weights, eye-dependent fade and quantized local vertex attributes, and the apparent spherical Sun and Moon. The grass query follows the GPU deformation model, subject to finite GPU arithmetic/rasterization differences. A celestial target is selected only if nearer than an existing Earth/grass hit. Body labels distinguish nominal/mean-radius spheres from terrain. These are apparent display-model ranges, not lunar topography or surface-specific light-time solutions. A ray that misses all measured surfaces reports **no surface hit**. Analytic queries use no arbitrary distance sentinel or far clipping plane; GPU presentation has separate finite clipping.

With a valid enabled atmospheric profile, the dot instead follows the shared curved-ray model. The displayed geometric path is the integral of arc length; phase optical path integrates refractive index, and a straight chord is separately defined. The first ellipsoid crossing blocks the sky. An escaping ray continues through vacuum to the existing apparent Sun/Moon spheres. Decorative grass is excluded in this optical mode. See [refraction.md](refraction.md) for data mapping, equations and error limits.

## Heading-specific skyline

With refraction disabled, the skyline readout measures the upper angular outline in the vertical plane of the current heading, using the current eye origin. It solves the analytic WGS84 tangent and reports the corresponding eye-origin range. It is independent of pitch, image resolution and telescope zoom. It does not mean the farthest visible object or atmospheric visibility.

Regional material boundaries cannot hide or raise the geometric horizon. The archived terrain solver retains separate finite-search limitations for historical tests; it is not the current runtime model. See [visibility.md](visibility.md).

## Projection and telescope

Rectilinear mode is the ordinary pinhole perspective model. Its image radius is proportional to `tan(θ)` and unrefracted straight world lines project to straight image lines. Optional atmospheric bending changes the optical path before that camera projection and can alter apparent shapes. It has no fisheye lens distortion, although any wide rectilinear view has perspective stretching near the edges.

Fisheye mode is an ideal **equidistant** projection, with radius proportional to `θ`. It uses a circular aperture and a maximum full vertical field of view of 180°. The zero-distortion form of the [OpenCV fisheye model](https://docs.opencv.org/4.13.0/db/d58/group__calib3d__fisheye.html) provides an independent description of this mapping.

The telescope uses the same eye position and central ray. Zoom narrows field of view; it does not change the modeled intersection point or invent distant detail. Fields of view are bounded below by 0.000001° for finite numerical behavior. Rectilinear field of view is bounded above by 150°. Apparent center magnification is relative to a 60° view of the same projection; rectilinear uses the ratio of half-angle tangents, and equidistant uses the ratio of fields of view.

GPU rendering uses the native display resolution, with finite single-precision camera transforms and rasterization. The retained analytic ray helpers also accept an explicit display aspect. Fisheye presentation reprojects six perspective views into the equidistant mapping; face resolution and interpolation remain finite.

## What is and is not accurate

- Geometry, collision and Earth range consistently use the smooth analytic WGS84 ellipsoid with f64 arithmetic. This is deliberately not topographic Earth or physical sea level.
- WBM classes and reconstructed borders retain finite native spacing, source epoch and uncertain real-world location. Outside coverage and in mixed unresolved cells, no category is inferred.
- The geoid readout is independent; h=0 corresponds to EGM2008 height −N, not generally zero.
- Materials and grass are decorative. Display chords, shadows, rasterization and finite precision do not add measured source detail.
- Optional [weather-informed refraction](refraction.md) traces curved rays through an explicitly transplanted NOAA forecast column. Curved path, phase optical path and chord are distinct; neither forecast resolution nor solver tolerances guarantees a real mirage. Tides, seabed, plate motion and datum epoch transformations remain absent.
- “100% physical accuracy” and unlimited telescope resolution cannot be guaranteed. Active and historical checks are distinguished in [verification.md](verification.md).

## Astronomical time and sky

Sun/Earth/Moon positions use the bundled JPL DE440s ephemeris over 1850–2149. Earth orientation combines IAU precession/nutation with observed or predicted IERS Earth rotation where covered; otherwise an explicitly labeled ΔT model is used. The continuous TT clock handles historical UTC offsets and leap seconds, with modeled UT1 labels before UTC existed and held future leap offsets. Apparent positions use the actual eye location, converged light time and special-relativistic aberration. [astronomy.md](astronomy.md) gives the complete time scales, frame conventions, physical sizes, orientation models and finite accuracy limits.
