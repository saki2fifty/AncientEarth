# Weather-informed atmospheric optics

Atmospheric refraction is an optional optical mode above the unchanged smooth WGS84 surface. It traces curved viewing rays through a vertical refractive-index profile derived from a real NOAA GFS forecast column. Turning it off restores geometric viewing. Collision, feet placement, land/water boundaries and the ellipsoid itself do not bend or move. This is a weather-informed simulation, **not an exact reconstruction of today's atmosphere or a guaranteed prediction of a real mirage**.

The implementation is in [`atmosphere.rs`](../../crates/earth-math/src/atmosphere.rs), [`weather.rs`](../../crates/earth-math/src/atmosphere/weather.rs) and [`refractivity.rs`](../../crates/earth-math/src/atmosphere/refractivity.rs). GPU presentation and guide searches have additional finite limits described in [rendering.md](rendering.md) and [horizon-guides.md](horizon-guides.md).

## What the downloaded weather means

The provider is NOAA/NCEP's GFS 0.25-degree GRIB2 product. The request selects temperature `TMP`, specific humidity `SPFH`, geopotential height `HGT`, and pressure `PRES`; the [official field inventory](https://www.nco.ncep.noaa.gov/pmb/products/gfs/gfs.t00z.pgrb2.0p25.f000.shtml) identifies their units and levels. Actual **surface pressure** is used, not pressure reduced to mean sea level. Pressure-level triplets supply the vertical structure; surface geopotential and 2m air temperature/specific humidity provide the lower reference.

The selected column is the nearest quarter-degree latitude/longitude node. This is a declared mapping of the provider's spherical-grid geographic labels onto the application's WGS84 geographic labels, not a datum transformation certified by the provider. The same column is extended horizontally along the entire optical path. It does not measure conditions along that path, interpolate adjacent weather columns, resolve individual buildings or provide centimetre-scale air layers above sand or water.

Live requests use the computer's current UTC, independently of the astronomical calendar. The nearest forecast valid hour is selected, and the newest requested six-hour cycle is conservatively at least six hours old. Two older cycles are attempted if needed. Cycle, valid time, source location, cache status and errors remain visible. A profile is eligible only at the same selected grid node and within three hours of its valid time. Moving to another node or expiration removes it until validated replacement data arrive. Download failure may retain an already valid profile, with its existing timestamp, but never fabricates weather. The explicit `--weather-reference` verification option uses a fixed NOAA snapshot; it is labeled as such and is never a live-network fallback.

The decoder checks the requested run/forecast identity, grid-post registration, NOAA/NCEP center, instantaneous product template, finite required values, duplicates, altitude bounds and complete vertical coverage. Pressure levels below the provider's ground are excluded. A missing, malformed or unsupported profile cannot silently turn into a nominal atmosphere. The downloaded fields are forecast model output, not a weather-station observation at the eye.

## Transplanting the column onto the flat-height world

GFS geopotential height `H` is not WGS84 ellipsoidal height. We use the conventional geopotential-to-geometric conversion from the [U.S. Standard Atmosphere 1976](https://www.ngdc.noaa.gov/stp/space-weather/online-publications/miscellaneous/us-standard-atmosphere-1976/us-standard-atmosphere_st76-1562_noaa.pdf), then subtract the provider ground's converted height:

```text
R₀ = 6,356,766 m
z(H) = R₀ H / (R₀ − H)
model profile height hᵢ = z(Hᵢ) − z(H_ground)
```

This conventional spherical-gravity conversion is an approximation, not an exact latitude-dependent gravity/geoid transformation. Its purpose is explicit: preserve the source column's height above its model ground while relocating that ground to the application's `h=0` ellipsoid. It restores neither topography nor geoid-shaped water. Eye height in the ray solver is the current WGS84 geodetic height, including flight altitude and crouch offset.

Between model ground and 2m, temperature and specific humidity are assumed equal to the supplied 2m values. The 2m pressure is an isothermal hydrostatic estimate from actual surface pressure:

```text
Tᵥ = T₂ₘ (1 + 0.6078 q₂ₘ)
p₂ₘ = p_surface exp[−9.80665 × 2 / (287.05 Tᵥ)]
```

Here `q` is water-vapor mass divided by total moist-air mass. The virtual-temperature expression is a conventional ideal moist-gas approximation; it is not a second independent weather measurement. This unresolved lower layer is particularly important for mirages. No invented heated-ground or inversion layer is added to make a dramatic picture.

## Refractive index and field

The moist-air **phase** index follows the Ciddor calculation documented by [NIST, Appendix A21–A41](https://emtoolbox.nist.gov/Wavelength/Documentation.asp). The rendered model uses a single vacuum wavelength of **550nm** and a fixed dry-air CO₂ assumption of **450µmol/mol**. Specific humidity is converted directly to vapor mole fraction using the dry-air and water molar masses; an RH saturation/enhancement correction is not applied again. This avoids ambiguity about whether subfreezing relative humidity refers to liquid water or ice.

```text
x_v = (q/M_water) / [(q/M_water) + (1−q)/M_dry]
n−1 = (ρ_dry/ρ_dry,reference)(n_dry,reference−1)
    + (ρ_vapor/ρ_vapor,reference)(n_vapor,reference−1)
```

The source implementation includes the published wavelength dispersion and moist-air compressibility terms. Its numeric acceptance range, 100–400K and 0–120000Pa, is deliberately broader than the cited metrology validation range. Upper-atmospheric samples are therefore **extrapolation**, explicitly flagged in metadata. Published NIST uncertainty figures are not an uncertainty bound for this forecast column or its rendered horizon.

At each mapped height we calculate `n−1`, then linearly interpolate that quantity between knots. The field depends on WGS84 geodetic height alone: `n(x)=n(h(x))`. Above the highest retained pressure level, a declared linear continuation reaches `n=1` at 120km above model ground, where it joins vacuum continuously. This is a computational upper boundary, not a measured thermosphere model. The lower and upper assumptions, coarse source levels and ignored horizontal variability prevent claims of exact local mirages. WMO identifies mirages with curved rays, inversions and potentially multiple images; resolving those real conditions requires more detailed atmospheric evidence. [WMO International Cloud Atlas: Mirage](https://cloudatlas.wmo.int/en/mirage.html).

## Curved-ray integration and distances

For an isotropic stationary medium, unit propagation direction `u`, geometric arc length `s`, and optical path `ℓ`, the geometric-optics equations are:

```text
dx/ds = u
du/ds = [∇n − u(u·∇n)] / n
dℓ/ds = n
∇n = (dn/dh) U_geodetic
```

The last identity uses the outward unit normal of the actual constant-geodetic-height surfaces. It does not replace them with spheres or ellipsoids having axes `(a+h,b+h)`. The equations follow expansion of `d(nu)/ds=∇n`; see [Bourgoin, Zannoni and Tortora, equations 1–5](https://arxiv.org/pdf/1901.08461). Reciprocity permits tracing from the eye toward a surface or the sky. The solver retains altitude turning points; it does not assume that every ray ascends or descends monotonically.

The f64 measurement solver uses explicit midpoint integration with step doubling. Position, direction-scaled-by-step and optical-integral discrepancies all participate in the local error estimate. Its local target is 0.001m, maximum step 2000m, maximum 4096 attempts, and an atmospheric path budget of 2000km checked between steps. A terminating step may extend past that path budget by up to one step. These are finite numerical controls, **not a global trajectory error or physical accuracy guarantee**. Direction is normalized during integration. Conservative height-excursion bounds also limit steps at each refractivity knot, so a thin layer cannot be silently skipped just because all midpoint probes missed it. The CPU knot band is 0.00001m; the GPU uses a larger scale-dependent representability band and rejects unresolved layer spacing. A curvature/height bound reduces steps near the ellipsoid; ground and atmospheric-exit events are refined by reintegration and bisection. A sphere bounds entry searches from space, but the accepted entry is on the actual constant-geodetic-height boundary.

The first resolved `h=0` crossing determines the surface and source-backed category. It is not a straight-ray intersection disguised by a screen warp. If the ray escapes, its outgoing direction continues through vacuum to the same apparent spherical Sun/Moon model used by astronomy. A ground hit blocks that continuation. From space, the initial vacuum segment is tested first, so a nearer lunar sphere cannot be hidden by a later Earth hit. Numerical/path-budget exhaustion returns **unresolved**, not a fabricated ground hit or silent geometric substitute.

Three lengths have distinct meanings:

- **Curved geometric path:** `∫ds`, the distance along the computed viewing ray.
- **Straight chord:** `|target−eye|`, which ignores ray bending.
- **Phase optical path:** `∫n ds`; this is not automatically a pulsed-laser time-of-flight range, which needs group-index treatment.

The heading-specific refracted skyline searches actual curved-ray hit/miss outcomes, first at 5-degree spacing and then by 22 local bisections. It reports the nearest surface hit on the highest detected branch. A narrower separate duct/image branch can be missed; the final local angular bracket does not certify the global skyline or its range. L/V likewise use curved-ray endpoints under their documented finite screen search. V traces the selected guide pixel to the camera-facing plane at the actual curved skyline hit, rather than intersecting a straight substitute ray with that plane.

## Display and remaining limits

Enabled refraction with a valid profile and WGS84 eye height from 0 through 20km uses a dedicated curved-ray surface/sky pass. The 20km operational limit is inclusive and applies to the eyes, not the feet. Above it, rendering, range, skyline and L/V consistently use geometric mode with an explicit status; the downloaded profile is retained. This conservative display limit does not truncate the atmosphere at 20km or restrict the f64 reference solver. It cannot be reproduced correctly by uniformly shifting an existing raster image. The GPU samples the same physical profile and analytic ellipsoid, with independently tested observer-relative f32 geometry and a separate finite display error/budget. Unresolved pixels are identifiable. Source-backed categorical triangles remain the land/water lookup; they are not converted into photographic imagery. Decorative grass is explicitly hidden in this optical mode because its ordinary raster geometry is not included in this first curved-ray renderer.

The atmosphere is horizontally stratified, frozen for each selected forecast snapshot, monochromatic and isotropic. Wind advection, turbulence, scintillation, chromatic dispersion in the final image, extinction, clouds, aerosol scattering, refractive illumination/shadows and exact radiometry are not solved. Artistic surface lighting and sky response do not become meteorological observations merely because viewing rays bend. Optical eligibility is not a prediction of whether haze, cloud, darkness or contrast lets a human see a target.

Independent checks use published NIST cases, exact radial optical integrals, straight-ray recovery, conserved equatorial Snell invariants with separate quadrature, and GDAL-decoded NOAA values. [`verify_weather.py`](../../scripts/verify_weather.py) reproduces the source fixture without using the production Rust decoder or weather formulas. [verification.md](verification.md) records actual completed checks and their limits.

The optional source-verification environment uses the existing pinned
[`requirements-terrain.txt`](../../scripts/requirements-terrain.txt) tools. For
the repository's local Windows installation:

```powershell
$env:PYTHONPATH = 'D:\_Codex\AncientEarth\.cache\python'
python scripts/verify_weather.py --check
cargo test -p earth-math --test refraction_review --locked --offline
```

Adjust the absolute Python path for another checkout, or install the pinned
requirements into a separate environment. GDAL is a verification tool only;
runtime ingestion uses the Rust GRIB decoder.
