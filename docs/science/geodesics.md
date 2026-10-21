# Chord and WGS84 surface distance

The measurement guide distinguishes a straight three-dimensional chord between its measured Earth endpoints from a shortest reference-ellipsoid surface path. They answer different questions and generally have different lengths.

For endpoint ECEF vectors `A` and `B`, the chord is:

```text
chord_metres = |B − A|
```

The surface measurement takes each endpoint's geodetic latitude and longitude and projects it to ellipsoidal height `h=0`. It then solves the inverse geodesic problem on the same WGS84 ellipsoid used throughout the application. Both endpoint coordinates must lie in the supported finite domain; invalid inputs or non-finite solver results produce unavailable/error status instead of a substitute distance. Input height is validated but does not enter the surface-path calculation.

With meridional radius `M` and prime-vertical radius `N`, the ellipsoid surface metric is:

```text
N(φ) = a / sqrt(1 − e² sin²φ)
M(φ) = a(1 − e²) / (1 − e² sin²φ)^(3/2)
ds² = M(φ)² dφ² + [N(φ) cosφ]² dλ²
surface_distance = minimum integral(ds) over connecting surface paths
```

Angles in this expression are radians. The minimizing path is the ellipsoidal geodesic. It is not generally a circular arc or a great circle. At special endpoint configurations more than one minimizing path can exist, while the shortest distance is still well defined. [GeographicLib's author documentation](https://geographiclib.sourceforge.io/html/python/geodesics.html) defines the inverse problem and these degeneracies.

The implementation uses the [Rust GeographicLib port, version 0.2.7](https://github.com/georust/geographiclib-rs/tree/v0.2.7), based on [Karney's geodesic algorithms](https://arxiv.org/abs/1109.4448), with explicit project constants `a=6378137 m` and `f=1/298.257223563`. Its sixth-order series and safeguarded inverse solution cover poles and nearly/exactly antipodal endpoints; there is no Vincenty iteration followed by a silent spherical fallback. The MIT-licensed dependency's optional polygon-accumulation feature is disabled because the application only requests inverse distance. Software notices accompany distributions.

This is a **reference-ellipsoid path**. It does not follow regional terrain triangles, climb hills, include obstacles, measure travel distance along the displayed skyline, or estimate a road route. The chord can include endpoint terrain height while the geodesic explicitly removes it, so no universal chord-versus-geodesic inequality is claimed for arbitrary elevated endpoints. For endpoints both on the ellipsoid, a surface path cannot be shorter than their straight chord.

Guide endpoint selection and screen silhouette refinement have their own finite sampling limits; an accurate inverse geodesic cannot recover a skyline endpoint that the contour search missed. Decimal precision is display rounding, not survey certainty. Independent regression cases check analytic equatorial lengths, reference meridian distances, symmetry, pole/dateline/antipodal behavior, height invariance and invalid-input rejection. See [verification.md](verification.md) and [`geodesic.rs`](../../crates/earth-math/src/geodesic.rs).
