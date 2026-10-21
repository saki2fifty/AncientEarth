# Astronomical time, positions and orientation

AncientEarth evaluates a published ephemeris at the selected simulation instant. The supported calendar interval is **1850-01-01 through 2149-12-31**, inside the bundled DE440s kernel's coverage. Time can run forward or backward, pause, or advance by a specified number of SI seconds. A second-by-second interface does not imply second-level certainty in every historical or future astronomical event.

The source-backed calculations live in [`crates/earth-math/src/astro`](../../crates/earth-math/src/astro). The application loads the kernel and Earth-orientation records once and supplies the observer's current eye ECEF position. Astronomical evaluation remains offline at runtime. Optional live weather is a separate subsystem and does not change the ephemeris clock.

## Physical dimensions and sources

| Quantity | Adopted value | Meaning |
| --- | ---: | --- |
| Earth | WGS84 `a=6378137 m`, `1/f=298.257223563` | Smooth reference ellipsoid; regional boundaries change categories only |
| Sun radius | `695700000 m` | IAU nominal solar-radius conversion constant, adopted as a spherical display model |
| Moon radius | `1737400 m` | Mean spherical lunar radius; no lunar topographic model |
| Light speed | `299792458 m/s` | Exact SI value |

The Sun's nominal radius comes from [IAU 2015 Resolution B3](https://www.iau.org/common/Uploaded%20files/IAUGA2015-Resolution-B3-recommended-nominal-conversion.pdf); the resolution distinguishes this exact **nominal conversion constant** from a time-varying measured solar photosphere. Lunar mean radius is consistent with [JPL satellite physical parameters](https://ssd.jpl.nasa.gov/sats/phys_par/) and the adopted [NAIF text PCK](https://naif.jpl.nasa.gov/pub/naif/generic_kernels/pck/pck00011.tpc). Earth's defining geometry and geodetic coordinates remain those in [model.md](model.md).

Planetary positions use the unmodified **JPL DE440s** SPK file, the compact time-span subset of DE440. [Park et al., 2021](https://ssd.jpl.nasa.gov/doc/de440_de441.html) describe the numerically integrated, observation-fitted dynamical model. The app evaluates its stored polynomials; it does not approximate these orbits as circles or independently integrate a few gravitational bodies. [NAIF's published rules](https://naif.jpl.nasa.gov/naif/rules.html) permit commercial use and redistribution of these unchanged kernels. Asset hashes and notices are recorded in the project license inventory.

## Calendar, UTC, TT and TDB

The calendar is proleptic Gregorian. Input accepts `YYYY-MM-DD HH:MM:SS[.fraction]`, optionally using `T` and a trailing `Z`. It has no local time-zone or daylight-saving conversion.

The clock stores **TT seconds relative to JD2451545.0 TT**, avoiding repeated arithmetic on a large single Julian-date number. Advancing one unit adds one continuous SI second. Display Julian dates are convenience values and are not reused as the high-precision clock state.

```text
TT = TAI + 32.184 s
TAI = UTC + ΔAT                  # where UTC is historically defined
TDB = TT + periodic_geocentric_correction
```

The `ΔAT` table includes the frequency-offset/drift segments of 1960-1971 and published positive leap seconds from 1972 onward, using the historical constants in [ERFA's `dat` routine](https://github.com/liberfa/erfa/blob/v2.0.1/src/dat.c). A published `23:59:60` is a distinct interval in the continuous clock. Invalid leap-second dates are rejected. Display fractions are truncated to milliseconds rather than rounding an ordinary `59.999...` into an invented leap-second label.

[IERS Bulletin C 72](https://datacenter.iers.org/data/html/bulletinc-072.html) confirms no leap second at the end of December 2026. Dates from 2027 onward hold the last published `TAI−UTC=37 s` and explicitly report that future leap seconds are unknown. The program does not invent a future leap schedule.

UTC did not exist before 1960. Earlier entered calendar labels mean **modeled UT1**: the historical/predictive [Espenak-Meeus ΔT polynomials published by NASA](https://eclipse.gsfc.nasa.gov/SEcat5/deltatpoly.html) provide `TT−UT1`. The interface labels this convention; it is not backdated UTC or an exact historical clock reconstruction.

TDB conversion uses the full **787-term Fairhead-Bretagnon geocentric series**, with the JPL-mass adjustments in [ERFA `dtdb`](https://github.com/liberfa/erfa/blob/v2.0.1/src/dtdb.c). The 474, 205, 85, 20 and 3 periodic terms multiply successive powers of Julian millennia:

```text
τ = TT_seconds_since_J2000 / (365250 × 86400)
TDB−TT = Σ(k=0..4) τ^k Σ(j in order k) A_j sin(ω_j τ + φ_j)
         + JPL_mass_adjustments
```

This is the geocentric time argument: ERFA's site-dependent `u,v` terms are zero. The app does not model a separate relativistic clock carried by the flying observer.

## SPK interpolation and center chains

The dependency-free reader supports the actual bundled format: little-endian IEEE DAF/SPK, Type 2 position polynomials, J2000/ICRF-aligned axes. It validates segment bounds, record dimensions, type/frame identifiers and finite evaluated states. Unsupported formats fail instead of silently changing algorithms.

Each segment covers an interval and identifies a target and center. Within a selected record:

```text
x = (TDB_seconds − record_midpoint) / record_radius
T₀(x)=1, T₁(x)=x, Tₙ(x)=2xTₙ₋₁(x)−Tₙ₋₂(x)
position_axis = Σ cₙ Tₙ(x)
velocity_axis = Σ cₙ T′ₙ(x) / record_radius
```

The file's kilometres and kilometres/second are converted to metres and metres/second. Endpoint selection includes the final record, and later applicable segments take precedence. These conventions follow the [NAIF SPK specification](https://naif.jpl.nasa.gov/pub/naif/toolkit_docs/C/req/spk.html).

The Sun is body 10 relative to the solar-system barycenter 0. Earth 399 and Moon 301 are stored relative to Earth-Moon barycenter 3; the reader adds the `3→0` state. Subtracting Earth's barycentric state then produces simultaneous geocentric positions. Treating the lunar or terrestrial `→3` record as a barycentric state would give incorrect geometry.

## Earth rotation and terrestrial axes

The inertial-to-Earth-fixed matrix follows the CIO formulation in [IERS Conventions, Chapter 5](https://iers-conventions.obspm.fr/content/chapter5/icc5.pdf):

```text
celestial_to_terrestrial = polar_motion × Rz(ERA) × celestial_to_intermediate
ERA = 2π [0.7790572732640 + 1.00273781191135448 × UT1_days_since_J2000]
      modulo 2π
```

The implementation separates fractional days before angle reduction. Bias/precession uses IAU 2006 Fukushima-Williams angles. Nutation uses the 77-term IAU 2000B series with the small P03 obliquity/J2 compatibility factors. The IAU 2006 CIO locator supplies `s`; polar motion uses IERS `xp,yp` and `s′`. These routines and unchanged coefficient tables are adapted from **ERFA 2.0.1**, under its retained BSD license. They are derived from SOFA through ERFA, not the unmodified SOFA reference implementation.

The abridged nutation model's approximately milliarcsecond performance is documented for 1900-2100; that is not an all-input accuracy guarantee or a claim for the wider application interval. Observed celestial-pole offsets and subdaily tidal Earth-orientation corrections are omitted.

The pinned, redistributable [Astropy distribution of `finals2000A.all`](https://github.com/astropy/astropy-iers-data/blob/798223ef83d161f89f7feb0b75f7a212de6c0353/astropy_iers_data/data/finals2000A.all) supplies daily observed/predicted `UT1−UTC`, `xp` and `yp`. Interpolation uses **`UT1−TAI` on the continuous TT axis**, then restores the appropriate scale; this avoids interpolating through a leap-second discontinuity. Observed and predicted rows have distinct quality labels. Outside coverage, NASA's ΔT model supplies UT1 and polar motion is zero. A change from measured data to this model can introduce a boundary discontinuity; extrapolation is not certified Earth rotation.

## Apparent sky, orientation and phase

The frame retains both simultaneous geometric centers and observer-dependent apparent centers. With observer barycentric position `O(t)`, the emission time satisfies:

```text
light_time = |target(t−light_time) − O(t)| / c
```

The solver iterates the actual ephemeris, then includes special-relativistic observer aberration. For unit source direction `n`, velocity ratio `β=v/c` and `g=sqrt(1−|β|²)`:

```text
n_apparent = [g n + β(1 + (β·n)/(1+g))] / (1+β·n)
```

For Earth-following eyes, observer velocity combines Earth's barycentric velocity and terrestrial spin at the eye. F9's Earth-centered nonrotating eye instead has zero held geocentric velocity, so it uses Earth's barycentric translation alone. Artificial flight speed is not treated as a physical relativistic boost. Small precession/polar-motion velocity derivatives and gravitational deflection of light are omitted. The same terrestrial rotation matrix transforms positions and directions; using the actual eye position includes topocentric parallax. [Reference frames](reference-frames.md) documents full-pose tracking, roll, time reversal and the explicit collision policy.

Sun and lunar body axes use the IAU coefficients in `pck00011.tpc`. The lunar pole and prime meridian include all 13 listed periodic arguments, evaluated at the retarded lunar emission time. This captures the text-PCK libration model but is less accurate than a matching binary lunar orientation kernel. The Sun uses the PCK reference rotation, not a model of differential rotation or individual sunspots.

Lunar illumination evaluates the Sun at its own retarded time relative to the lunar emission event. With unit Moon-to-Sun direction `s` and Moon-to-observer direction `o`, geometric illuminated fraction is `(1+s·o)/2`. Surface brightness, opposition surge, detailed albedo and solar limb structure are outside this astronomical model. Optional [atmospheric refraction](refraction.md) traces viewing rays through a weather-informed profile before their vacuum continuation reaches these same apparent body spheres; the ephemeris positions themselves remain unrefracted. Display materials and shadow maps introduce their own finite-resolution approximations; see [rendering.md](rendering.md).

Solar eclipses now use the same receive-time apparent spheres at the eye, plus a spatial light-time field for surface shading. Angular radii are `asin(radius/distance)`; separation uses `atan2` of normalized-vector cross length and dot product. A stable circle-overlap calculation yields geometric disk obscuration and distinguishes no overlap, partial coverage, annularity and totality. This is a smooth-body, small-angle area model with a first-order spatial ephemeris extension, not certified contact timing or limb-darkened irradiance. [The eclipse model](eclipses.md) gives the complete equations, primary sources, historical checks and limits.

The center-dot query intersects the apparent nominal Sun and mean-radius Moon spheres with stable forward quadratic roots. Directions are normalized, inside-sphere rays can select the forward exit, and a celestial hit is returned only when nearer than an existing Earth/grass hit. The reported surface is the **apparent spherical display model**: it is not lunar terrain or a per-surface-element retarded light-time solution. In dimensionless coordinates `o=(eye−center)/radius`, the solver uses `B=o·D`, `C=(|o|−1)(|o|+1)`, `Δ=1−|o×D|²`, stable roots `q` and `C/q`, and restores metres by multiplying by radius. Its numerical grazing band matches the ellipsoid ray routine.

## Verification and regeneration

Independent tests compare native SPK states and body orientations against CSPICE, UTC/TT and TDB conversion against ERFA, and complete Earth-orientation matrices against a separately assembled Python ERFA call chain. They also test leap boundaries, invalid/extreme inputs, record endpoints, center chains, posture and frame geometry.

[`verify_astronomy.py`](../../scripts/verify_astronomy.py) regenerates the independent CSPICE/ERFA state, clock, body-orientation and apparent-position fixtures. [`prepare_tdb_coefficients.py`](../../scripts/prepare_tdb_coefficients.py) and [`prepare_orientation.py`](../../scripts/prepare_orientation.py) retain coefficients from pinned ERFA source; their `--check` modes verify generated files. [`verify_astronomy_orientation.py`](../../scripts/verify_astronomy_orientation.py) regenerates six independent matrix fixtures across the supported interval. Optional reference tools are development-only. Test tolerances demonstrate agreement for specified cases; they do not eliminate ephemeris uncertainty or establish exact physical positions for every date.
