# Real terrain, elevation datum and the canonical surface

> **Historical pipeline, not the active surface.** Current runtime uses a smooth WGS84 ellipsoid at h=0 everywhere and WBM-only category borders. DSM elevations, geoid surface displacement and photographs described below are archived and not loaded. See [current model](model.md) and [boundary preparation](water.md).

The app includes four finite regional meshes derived from **Copernicus DEM GLO-30 Public, AWS 2021 release**: Mount Fuji, Tokyo, Houston and Galveston. Each covers approximately 20 km east–west and north–south. Matching licensed Water Body Masks classify ocean, lake and river coverage. Ocean shores additionally use native source posts and reconstructed contour vertices, preserving source-post heights while refining between-post interpolation; [water.md](water.md) describes the shared topology, EGM2008 boundary assumption and finite shoreline resolution. Outside imported coverage, the active surface is explicitly the WGS84 reference ellipsoid. The terrain/water checkbox selects either this combined scene or the reference ellipsoid everywhere.

## Source meaning and vertical datum

Copernicus is a digital **surface** model (DSM), including effects of vegetation, infrastructure and buildings. It is not a bare-earth survey or a collection of individual building meshes. Its DGED horizontal coordinates use WGS84-G1150 and its elevations use EGM2008 height. The [publisher's specification](https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM) states 1 arc-second latitude spacing for GLO-30 and source-product absolute errors below 4 m vertically at 90% linear confidence and below 6 m horizontally at 90% circular confidence. Those statistical product specifications do not bound every sample or the resampled application mesh.

The imported release and its processing are documented by the [AWS dataset readme](https://copernicus-dem-30m.s3.amazonaws.com/readme.html). The full-resolution bands are read; averaged overview bands are not used. Dates and any source gap filling remain properties of the upstream product. The app does not perform a realization/epoch transformation between WGS84-G1150 and a current GPS realization.

WGS84 geodetic conversion requires ellipsoidal height `h`, whereas the source supplies EGM2008 height `H`. The preprocessing script samples NGA's EGM2008 2.5 arc-minute geoid grid distributed by PROJ and applies:

```text
h(φ,λ) = H_EGM2008(φ,λ) + N_EGM2008(φ,λ)
```

Here geoid undulation `N` is positive when the geoid is above the ellipsoid. It is unrelated to the prime-vertical curvature radius also conventionally named `N`. The sign follows the [PROJ geodetic-grid convention](https://proj.org/en/stable/specifications/geodetictiffgrids.html) and [GeographicLib's geoid height relation](https://geographiclib.sourceforge.io/html/GeoidEval.1.html). The exact downloaded grid, CRS metadata and file hashes appear in each provenance file. Geoid interpolation is bilinear between registered grid posts:

```text
N(x,y) = (1−x)(1−y)N00 + x(1−y)N10 + (1−x)yN01 + xyN11
```

`x,y` are fractional column/row positions in the surrounding geoid cell. Five positions per patch are compared to an independent PROJ vertical-grid operation. Agreement checks interpolation and sign; it does not eliminate uncertainty in the geoid model or DEM.

The live vertical HUD uses the same native global geoid source, bundled losslessly for queries anywhere on Earth. It computes `H = h − N` at the observer's latitude/longitude and finds the ground beneath along geodetic up. [geoid.md](geoid.md) distinguishes this modeled sea-level reference from instantaneous tide. Regional masking and negative coastal terrain also affect the skyline; [visibility.md](visibility.md) describes actual mesh sections and the explicitly estimated boundary case.

## Registration and retained samples

[`prepare_terrain.py`](../../scripts/prepare_terrain.py) checks CRS, north-up affine registration, point sampling, finite data and the complete interpolation neighborhood. Invalid or missing heights cause failure rather than a zero-height replacement. GDAL already handles the `RasterPixelIsPoint` offset: sample `(column,row)` uses the returned affine at `(column+0.5,row+0.5)`, exactly once. This follows [GDAL RFC 33](https://gdal.org/en/stable/development/rfc/rfc33_gtiff_pixelispoint.html).

The approximately 20 km request uses local WGS84 curvature at its center:

```text
M = a(1−e²)/(1−e²sin²φ)^(3/2)
N = a/sqrt(1−e²sin²φ)
half latitude span  = L/(2M)
half longitude span = L/(2Ncosφ)             # radians; L = 20,000 m
```

This is a local sizing approximation, not an assertion of exact rectangular geodesic distances. The crop moves inward to source row/column indices divisible by three. Every third original height is retained without averaging; geoid correction is then added. Resulting spacing is 3 arc seconds: approximately 92 m north–south, 76 m east–west in Japan and 81 m east–west in Texas. Exact center spacings are recorded in provenance. Source points between retained posts, including peaks, can disappear.

The following table describes the unchanged **base grid**. The active coastal companions append vertices and replace selected base faces; their complete face counts and construction recipes are recorded in the water provenance.

| Patch | Base posts | West / east | South / north | Base triangles |
|---|---:|---|---|---:|
| Fuji | 265 × 217 | 138.62° / 138.84° | 35.27° / 35.45° | 114,048 |
| Tokyo | 265 × 216 | 139.58166666666665° / 139.80166666666665° | 35.6° / 35.77916666666667° | 113,520 |
| Houston | 249 × 217 | −95.46666666666667° / −95.25999999999999° | 29.673333333333332° / 29.85333333333333° | 107,136 |
| Galveston | 247 × 216 | −94.89999999999999° / −94.695° | 29.211666666666666° / 29.390833333333333° | 105,780 |

Exact coordinates, source and output SHA-256 hashes, height ranges, actual center-line geodesic extents and independent interpolation checks are retained in [Fuji](../../assets/terrain/fuji.provenance.json), [Tokyo](../../assets/terrain/tokyo.provenance.json), [Houston](../../assets/terrain/houston.provenance.json) and [Galveston provenance](../../assets/terrain/galveston.provenance.json).

The Texas request centers and ground presets preserve the cached GeoNames source points: Houston `(29.76328°,−95.36327°)`, ID 4699066; Galveston `(29.30135°,−94.7977°)`, ID 4692883. They are representative places, not unique geographic definitions of the cities. The aerial presets look toward those points from 1,200 m above the mesh.

Coastal ellipsoidal heights must remain negative when warranted. The imported Houston posts range from approximately −28.3984 to +32.4304 m; every Galveston post is below the ellipsoid, approximately −31.2803 to −7.7430 m. This is compatible with positive or near-zero source elevation above the geoid because local geoid undulation is negative. Water posts describe the source DSM water surface, not bathymetry or current tidal sea level.

## One mesh for display, range and positioning

Every retained `(latitude,longitude,ellipsoidal height)` becomes an `f64` ECEF vertex using the equations in [model.md](model.md). Each cell has a fixed northwest–southeast diagonal, forming triangles `(NW,SW,SE)` and `(NW,SE,NE)`. Their interiors are **planar in ECEF**:

```text
P = (1−u−v)A + uB + vC
u ≥ 0; v ≥ 0; u+v ≤ 1
normal = outward-oriented normalize((B−A) × (C−A))
```

The canonical measurement/collision model has no separate bilinear height surface, procedural mountain or coarser collision mesh. It is a numerical approximation between retained source posts. ECEF chords and geographic latitude/longitude lines are not identical. GPU presentation can select a coarser LOD and finite reference-ellipsoid tessellation; these display approximations do not change canonical measurements. See [rendering.md](rendering.md).

Ray directions are normalized, making the ray parameter Euclidean metres. Triangle intersection uses double-sided, sheared-coordinate edge functions following [Woop, Benthin and Wald's construction](https://jcgt.org/published/0002/01/05/paper.pdf). The largest direction component defines axis `kz`; the other axes are permuted and swapped when needed. For each vertex relative to the ray origin:

```text
x′ = x − (Dx/Dz)z; y′ = y − (Dy/Dz)z; z′ = z/Dz
U = Cx′By′ − Cy′Bx′
V = Ax′Cy′ − Ay′Cx′
W = Bx′Ay′ − By′Ax′
t = (UAz′ + VBz′ + WCz′)/(U+V+W)
```

Mixed edge signs reject an outside point. A zero denominator rejects a parallel/degenerate intersection. Finite `t ≥ 0` candidates are ordered by distance. Shared edges use the same vertex operations. This is a robust floating-point implementation, not an exact-arithmetic guarantee.

A median-split bounding-volume hierarchy (BVH) accelerates the same triangle tests. It does not change the surface. Bounds use conservative padding `10⁻⁶ m + 32ε max(|origin|,a)` and leaves contain at most eight triangles. Independent tests compare accelerated and brute-force traversal over 1,200 varied rays.

To determine ground at a geographic coordinate, the scene casts down the **geodetic normal** from above each patch's maximum height and takes the highest applicable mesh hit. That same actual mesh footprint masks the reference ellipsoid. A rectangular bounding-box mask would leave small edge inconsistencies because mesh edges are Cartesian chords. Both forward ellipsoid roots remain eligible outside coverage, even when the nearer root is masked by below-ellipsoid terrain.

Rendered pixels, the center-dot range, pin occlusion and movement collision all query this scene. Ground placement retains the requested latitude/longitude and recovers the mesh intersection's ellipsoidal height. Eye height is added along geodetic up, not along the tilted triangle normal. Thus looking straight down from a grounded observer measures the configured eye height; looking perpendicular to a slope measures a different range.

Exact coordinate entry is still **feet ellipsoidal height**, including negative values when above the active mesh. Destinations below the active surface or outside the observer domain are rejected without movement. The separate ground action explicitly snaps to the active surface. Collision sweeps the feet along the straight ECEF movement segment and stops before an intersection with 0.00001 m numerical clearance; endpoint ground validation also prevents entry below a discontinuous patch boundary. This is a point-feet navigation constraint, not a full human-body collision volume or gravity simulation.

## Coverage, appearance and uncertainty

Imported patches can meet the reference fallback with visible height discontinuities; no invented connecting terrain hides them. Ground queries use a 1,000 m margin above each patch's maximum source height and exclude opposite-side intersections. The loader accepts at most four million posts per patch, no antimeridian wrapping, spans at most 5° in either direction, and latitudes strictly inside ±89°. Stored vertex heights must be within −10,000 to +100,000 m. A conservative support-plane test also rejects a triangle unless its entire interior is above the supported −10,000 m inward parallel surface; valid post heights alone do not prevent Cartesian chords sagging below that domain.

The current renderer uses procedural materials and illustrative nearby grass, with no photographic imagery. Those visual details do not establish real land cover, reconstruct individual source buildings/trees, or increase DEM accuracy. The older GSI/NAIP/NASA photo renderer and its registration equations are archived in [imagery.md](imagery.md); retained photo assets are not used by the active application.

Numerical consistency is distinct from physical accuracy. DEM uncertainty, geoid uncertainty, realization/epoch differences, decimation and planar interpolation remain. No uniform metre-level real-world range guarantee follows from printing six decimals. Grazing rays are sensitive to small surface changes. Refraction, tides, vegetation motion, tectonic motion and detailed objects are outside this model.

## Reproduction

Install the pinned optional processing dependencies with `python -m pip install -r scripts/requirements-terrain.txt`; the native app does not require Python. Save these exact source files under `.cache/terrain`:

- [`N35E138.tif`: Fuji source COG](https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_N35_00_E138_00_DEM/Copernicus_DSM_COG_10_N35_00_E138_00_DEM.tif).
- [`N35E139.tif`: Tokyo source COG](https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_N35_00_E139_00_DEM/Copernicus_DSM_COG_10_N35_00_E139_00_DEM.tif).
- [`N29W096.tif`: Houston source COG](https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_N29_00_W096_00_DEM/Copernicus_DSM_COG_10_N29_00_W096_00_DEM.tif).
- [`N29W095.tif`: Galveston source COG](https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_N29_00_W095_00_DEM/Copernicus_DSM_COG_10_N29_00_W095_00_DEM.tif).
- [`us_nga_egm08_25.tif`: PROJ-distributed NGA geoid grid](https://cdn.proj.org/us_nga_egm08_25.tif).

Then run:

```powershell
python scripts/prepare_terrain.py
python scripts/verify_terrain.py
cargo test -p earth-math
```

The script emits `AETERR01` files: 8-byte magic, little-endian `u32` width/height, four little-endian `f64` values (west, north, positive longitude step, positive latitude step), then row-major ellipsoidal `f64` heights with rows progressing south. The header is 48 bytes. Length, dimensions, registration and heights are validated before building geometry. This is a developer import pipeline; the app does not automatically fetch worldwide terrain.

Use repeated `--patch` options to prepare a subset, for example `--patch houston --patch galveston`. The generated imagery plan distinguishes GSI XYZ tile mosaics from USGS NAIP registered image exports; providers are not interchangeable.
