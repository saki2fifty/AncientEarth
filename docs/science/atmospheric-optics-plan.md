# Atmospheric refraction and mirages: implementation assessment

**Historical planning assessment.** A bounded weather-informed curved-ray implementation now exists; [refraction.md](refraction.md) describes its actual data, assumptions, solver and limitations. The following proposal is retained as design history, not a current feature inventory.

## Required physical inputs

Air refractive index depends on wavelength, temperature, pressure, humidity and composition. NIST documents the Ciddor model, its inputs and uncertainty; it also distinguishes the phase index from the group index needed by some pulsed ranging applications. A single surface weather reading cannot specify a three-dimensional refractive-index field. [NIST air-index documentation](https://emtoolbox.nist.gov/wavelength/documentation.asp).

A useful first implementation would accept a declared vertical temperature/pressure/humidity profile, plus optional near-surface inversion or heated-surface layers. These would be **user-selected model conditions**, unless accompanied by appropriately licensed measurements. GPS coordinates and astronomical time alone cannot reconstruct the actual atmosphere. USNO explicitly notes that unpredictable near-horizon refraction limits real rise/set predictions. [USNO rise/set definitions and limitations](https://aa.usno.navy.mil/faq/RST_defs).

WMO describes mirages as curved-ray phenomena that can produce displaced, inverted or multiple images, including distorted Sun/Moon images and optical ducting. This is why a complete mirage model requires more than a single apparent-horizon angle. [WMO International Cloud Atlas: Mirage](https://cloudatlas.wmo.int/en/mirage.html).

## Ray model

For a stationary isotropic medium in geometrical optics, write position as `x(s)`, unit propagation direction as `u`, arc length as `s`, and phase refractive index as `n(x, wavelength)`. Expanding `d(nu)/ds = gradient(n)` gives

```text
dx/ds = u
du/ds = [gradient(n) - u dot(u, gradient(n))] / n
```

The direction derivative is perpendicular to `u`; the exact differential equation therefore preserves unit direction. Constant `n` recovers a straight ray. Integrating `n ds` gives phase optical path length. Euclidean endpoint distance, curved geometric path length and optical path length are different quantities and would need distinct readout labels. A time-of-flight model requires the appropriate dispersive/group-index treatment. These equations and the need to specify the spatial refractive field follow the geometric-optics framework in [Bourgoin, Zannoni and Tortora, *Analytical ray-tracing in planetary atmospheres*, equations 1–5](https://arxiv.org/pdf/1901.08461).

The proposed implementation would integrate backward from the eye with adaptive error control, locate the first ellipsoid or object intersection along the curved path, and retain turning rays. Restricting every direction to a single monotonic altitude correction would exclude important mirage behavior. A limited spherical-atmosphere solver can be a useful independent validation case, but spherical symmetry must not silently replace the project's WGS84 ellipsoid in the general model. [Thompson, Pepin and Simon, *Ray tracing in a refracting spherically symmetric atmosphere*](https://opg.optica.org/josa/abstract.cfm?uri=josa-72-11-1498).

## Integration with the current renderer

The following is an engineering proposal, not a claim about completed capabilities:

1. Implement and independently test an f64 reference ray solver with explicitly chosen atmospheric profiles. Validate straight-ray recovery, conserved direction norm, profile discontinuity handling, convergence, turning rays and known layered/spherical cases.
2. Make sight range, horizon guides, target selection and visible celestial directions use that same optical model. Preserve a switch to geometric, unrefracted measurements. Ordinary collision and the ellipsoid's shape remain geometric.
3. Add a GPU presentation path which samples the same profile and traces to the appropriate surface or sky direction. Bevy's current perspective rasterization assumes straight rays; a uniform image warp or effective-Earth-radius multiplier cannot generally recover finite-distance occlusion and multiple images. A depth-aware hybrid or curved-ray surface pass needs explicit handling of grass, shadowing and hidden surfaces.
4. Validate rectilinear, telescope and fisheye views against the CPU solver, including near-horizon multi-image cases. Establish runtime and numerical budgets before promising a frame rate. Refractive illumination, extinction/scattering, turbulence and eye adaptation are additional models, not automatic consequences of bending the viewing ray.

This is practical as a substantial new optical subsystem. A controlled-profile simulation can be scientifically testable; predicting the exact mirage at an arbitrary place and second would additionally require sufficiently resolved atmospheric observations. No new weather data, third-party code or raster assets were bundled for this assessment.
