# Solar eclipses: geometry, light time and limits

AncientEarth computes the fraction of the apparent solar disk covered by the Moon at the eye and at rendered surface positions. A total eclipse can produce **100% coverage of this model's solar disk**. That number is not a claim of 100% physical accuracy, exact contact times or measured illumination.

The implementation is [`astro/eclipse.rs`](../../crates/earth-math/src/astro/eclipse.rs). It uses the existing unmodified JPL DE440s ephemeris, WGS84 observer position, published Earth-orientation records and time-scale conversions described in [astronomy.md](astronomy.md). The Sun remains a sphere of nominal radius 695,700,000 m; the Moon remains a mean-radius sphere of 1,737,400 m. No new imagery or external art is involved.

## What the readout means

The readout is **geometric obscuration**, an area fraction. It is distinct from eclipse magnitude, which refers to the covered fraction of the solar diameter. The terminology follows [NASA's eclipse glossary](https://eclipse.gsfc.nasa.gov/SEhelp/SEglossary.html).

Let `α` and `β` be the apparent angular radii of the Sun and Moon and `δ` their angular separation:

```text
α = asin(R_sun / distance_sun)
β = asin(R_moon / distance_moon)
δ = atan2(|unit_sun × unit_moon|, unit_sun · unit_moon)

δ ≥ α+β                  no overlap
β ≥ α and δ ≤ β−α        total / umbra
β < α and δ ≤ α−β        annular / antumbra
otherwise                 partial / penumbra
```

The Moon must also be nearer than the Sun. An observer inside either adopted sphere is rejected. These are smooth-silhouette classifications, with ordinary floating-point uncertainty at exact contact. A zero-width external tangency has zero area.

Sun elevation is measured above the observer's local **geodetic horizontal**. It is not a terrain visibility result, refracted elevation or the depressed Earth limb seen from orbit. Eclipse geometry can be evaluated below that horizontal; ordinary surface orientation and nighttime lighting still determine whether direct sunlight reaches rendered ground.

## Receive-time geometry

Using simultaneous Sun and Moon centers would move the modeled event. Each target is first evaluated at its own converged **reception** light time for the current observer:

```text
τ_target = |target(t−τ_target) − observer(t)| / c
c = 299792458 m/s
```

This follows the reception geometry documented in [NAIF's occultation routine](https://naif.jpl.nasa.gov/pub/naif/toolkit_docs/C/cspice/occult_c.html). It does not trace a ray using a Sun position and a Moon position from one shared emission time.

For the eye, the eclipse field uses the same retarded centers and observer aberration as the displayed apparent body spheres. Aberration is the common relativistic direction transformation already documented in [astronomy.md](astronomy.md). The display keeps sphere radii and retarded distances; it does not separately transform every limb ray or solve a retarded event for every surface element. NAIF's general occultation routine omits stellar aberration because it does not improve the occultation classification. Here the same center-direction convention is retained to keep the eye readout and displayed disks consistent.

When F9 holds the eye in Earth-centered nonrotating axes, its aberration uses Earth's orbital translation without terrestrial co-rotation. The observer eclipse field follows that convention, while the independent surface field continues to use co-rotating ground velocities. [Reference frames](reference-frames.md) explains this distinction and the kinematic scope.

Rendering needs a spatial field across Earth, not a single brightness multiplier at the eye. At each new simulation sample, the field retains the anchor observer, target vectors `r₀` at their separate emission times, and target **barycentric** velocities `v` at those times, all expressed in common Earth-fixed axes. For a surface offset `p` from the anchor, it solves:

```text
Δτ = (|r₀ − p − v Δτ| − |r₀|) / c
r  = r₀ − p − v Δτ
```

Six fixed-point iterations update the light-time difference. To avoid subtracting large nearly equal lengths, the implementation sets `q=−p−vΔτ` and evaluates the numerator as:

```text
|r₀+q| − |r₀| = q·(2r₀+q) / (|r₀+q|+|r₀|)
```

This is a first-order extension of the ephemeris in **emission time**, not an exact resampling of its polynomials at every fragment. Barycentric target velocities are needed because reception remains at the same barycentric event while the target emission epoch changes. Observer velocity at another point is adjusted by `ω×p`; the artificial flying speed is excluded, as in the rest of the sky model. The field is rebuilt from the ephemeris as the simulation time or observer changes.

Fresh ephemeris samples at other locations provide an independent check of the spatial approximation. Their tested differences are implementation evidence, not a bound on every supported date, altitude or body approach.

At the tested 2024 epoch, 30 terrestrial points and anchors at ground level and 1,000,000 km altitude differed from fresh samples by at most approximately `4.805×10⁻¹² rad` in separation and `5.114×10⁻¹⁰` in obscuration. These are finite-case numerical comparisons, not physical accuracy estimates.

## Stable overlap calculation

The obscuration calculation treats the two angular silhouettes as circles in a plane. This is a **small-angle angular-disk area model**, not an exact intersection integral of spherical solid angles. The distinction is small for the roughly half-degree disks viewed from Earth, but the model is not promoted to exact radiometry or high-precision close-lunar observations.

Normalize the solar radius to one:

```text
d = δ/α
r = β/α
k = 1−r
a = [d + k(1+r)/d] / 2
h = sqrt(max(0, (1+r−d)(d−k)(d+k)(d+1+r))) / (2d)
A = atan2(h,a) + r² atan2(h,d−a) − d h
obscuration = clamp(A/π, 0, 1)
visible_fraction = 1−obscuration
```

The formula applies only to partial overlap. The disjoint and containment branches are evaluated first, avoiding division by zero at coincident centers. Annular containment gives `obscuration=r²`; total containment gives one.

`a` is the solar-center-to-common-chord coordinate and `h` is the chord half-height. Adding the two circular sectors and subtracting their triangles gives `A`. Factoring `1−r²` as `(1−r)(1+r)` and using `atan2` avoids the poor conditioning of `acos` near a rounded unit argument. Independent contact tests exposed visible artifacts in an earlier GPU expression; the factored chord form replaces it.

## GPU representation and lighting

Double-precision code constructs an orthonormal frame whose third axis points toward the retarded Sun. It transforms the target vectors and velocities into that frame **before** converting to float32. This preserves the Moon's comparatively small transverse coordinates. Three affine rows map the renderer's local positions into the same frame, including the current floating-origin offset.

The GPU performs the corresponding position-dependent reception correction, aberration, angular-size and overlap calculations. It uses `atan2` of the cross-product length and dot product for separation; `acos` of a nearly unit dot product would lose precision. Finite uniforms, vertex interpolation and GPU arithmetic still introduce numerical error, especially at boundaries. Numerical comparisons across locations and floating-origin changes test this representation.

A scalar float32 execution of the shader equations, using the actual uploaded field and local-position values, was checked at 504 point/rebase combinations over two eclipse epochs. The maximum sampled absolute visible-fraction difference from fresh float64 geometry was `2.268×10⁻⁷` for ground/8,000 km anchors and `1.946×10⁻⁵` for a 1,000,000 km anchor. These tests exclude native GPU fusion, transcendental implementation and raster interpolation; they are not universal error bounds. [Rendering details](rendering.md) distinguish direct-surface attenuation from the illustrative observer sky response.

Surface sunlight attenuation uses `visible_fraction` as a **uniform-disk lighting approximation**. Geometric area coverage is not a measured spectral irradiance reduction: solar limb darkening, atmosphere, clouds, wavelength dependence and multiply scattered light are absent. The renderer's sky response, residual fill and material exposure remain illustrative. Eclipse attenuation does not change the ellipsoid, grass geometry, collision or distance measurements.

The real corona is visible during totality because the bright photosphere is covered; [NASA's Sun description](https://science.nasa.gov/sun/facts/) distinguishes these structures. This model does not derive the corona, prominences or atmospheric brightness from the ephemeris.

## Test-only historical circumstances and validation

Named-location demonstrations and the runtime city catalog have been removed. Historical circumstance coordinates remain only in scientific test fixtures, with their source attribution retained. They are representative [GeoNames](https://www.geonames.org/) points, not surveyed eclipse sites or current navigation shortcuts:

| Test circumstance | Latitude, longitude | Calendar instant, UTC |
| --- | --- | --- |
| Total-eclipse fixture | 32.78306°, −96.80667° | 2024-04-08 18:42:00 |
| Annular-eclipse fixture | 35.08449°, −106.65114° | 2023-10-14 16:37:00 |
| Partial-eclipse fixture | 29.76328°, −95.36327° | 2024-04-08 18:40:00 |
| No-eclipse fixture | 29.76328°, −95.36327° | 2024-04-09 18:40:00 |

The active Earth surface is the smooth WGS84 ellipsoid at h=0. Eclipse physics remains available at user-entered coordinates and times. Observer altitude changes topocentric parallax; the view from orbit need not have the same eclipse fraction as the surface underneath.

Independent references are regenerated with [`verify_eclipses.py`](../../scripts/verify_eclipses.py), using original CSPICE reception calculations and ERFA time/orientation/aberration. Its area reference integrates vertical chords with adaptive quadrature instead of using the production sector expression. Fixture dates avoid leap boundaries; existing time tests cover those separately.

[`verify_eclipse_disk_error.py`](../../scripts/verify_eclipse_disk_error.py) independently compares the angular-plane area with spherical-cap intersection and Sun-normal projected uniform-radiance integrals. Across its 459 sampled geometries, the largest absolute fraction differences were approximately `8.775×10⁻⁷` and `1.280×10⁻⁶`, respectively. Doubling its quadrature from 256 to 512 nodes changed a result by at most `3.45×10⁻¹⁵`. This finite sweep quantifies the approximation on those cases; it does not certify an all-geometry maximum and does not include limb darkening or atmospheric effects.

Historical comparisons include NASA's WGS84 [2024 total-eclipse path table](https://eclipse.gsfc.nasa.gov/SEpath/SEpath2001/SE2024Apr08Tpath.html) and [2023 annular-eclipse path table](https://eclipse.gsfc.nasa.gov/SEpath/SEpath2001/SE2023Oct14Apath.html). Those tables use different ephemerides, Earth-rotation assumptions and adopted lunar limb radii. Their rounded path, diameter-ratio and duration values are cross-model checks, not exact numerical fixtures for DE440 and this model. [The verification record](verification.md) records the measured comparisons and tolerances.

Lunar topography, the difference between center of mass and center of figure, solar-radius convention, Earth orientation and missing terrain all limit physical contact predictions. [NASA's lunar-limb discussion](https://eclipse.gsfc.nasa.gov/SEhelp/limb.html) explains why a smooth mean Moon cannot reproduce precise observed contacts. The UI's second-scale clock and three-decimal percentage formatting do not remove those limitations.
