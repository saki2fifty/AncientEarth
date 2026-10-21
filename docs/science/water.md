# Height-free land/water boundaries on WGS84

The current application places **all land, ocean, lake and river surfaces on the analytic WGS84 ellipsoid at ellipsoidal height h=0**. The Copernicus Water Body Masks supply regional categories only. No DSM heights, geoid displacement, bathymetry or photographs enter boundary preparation or runtime surface geometry. Historical elevation assets and tests remain archived; they are not embedded in the current application.

## Sources, registration and rights

The [Copernicus product handbook, section 1.2.5.4](https://dataspace.copernicus.eu/sites/default/files/media/files/2024-06/geo1988-copernicusdem-spe-002_producthandbook_i5.0.pdf) defines `0=non-water`, `1=ocean`, `2=lake`, `3=river`. Categories are separate from the elevation band. Neither a zero height nor a blue material establishes real water coverage.

Four already reviewed WBM tiles are cropped using fixed source row/column bounds recorded in [boundary provenance](../../assets/boundaries). Each actual source is a 3600×3600 unsigned-byte EPSG:4326 north-up PixelIsPoint grid. The [AWS processing description](https://copernicus-dem-30m.s3.amazonaws.com/readme.html) describes shared-edge trimming; actual transforms and hashes are checked rather than inferred. The original categorical band is read without averaged overviews.

Rasterio/GDAL exposes a corner-based affine even for point rasters. Native post `(row,column)` is evaluated at `(column+0.5,row+0.5)`, with no additional half-pixel shift, following [GDAL RFC33](https://gdal.org/en/stable/development/rfc/rfc33_gtiff_pixelispoint.html). The retained base uses stride three, while ocean-boundary cells include native posts. The importer checks source SHA-256, CRS, registration, category domain, scale/offset and finite crop bounds. Its only raster input is WBM; DSM/geoid files are not opened.

The exact EULA was reviewed before use. Modification/redistribution rights and retained obligations are recorded in [the source review](../../licenses/water-sources.json), [original license](../../licenses/Copernicus-WorldDEM-30-eula_F.pdf), [NOTICE](../../NOTICE.md) and [asset inventory](../../licenses/assets.json). These permissions apply to the identified product files, not arbitrary public map downloads.

## Declared ocean-shore reconstruction

Each coarse quadrilateral covers a four-by-four native-post stencil. If ocean and another category both occur, it is refined into three-by-three native cells. This changes categorical interpolation only. Ordinary homogeneous cells remain coarse; unresolved inland-water mixtures retain class4 and do not imply land.

For an ordinary binary land/ocean cell, the category changes on exactly two of its four outer edges. Their midpoints define the reconstructed boundary endpoints. The cell's terrain diagonal is **not** an extra shoreline constraint: forcing a three-to-one category pattern through a diagonal midpoint created artificial right-angle steps in the earlier implementation. A checkerboard has four crossing edges and genuinely ambiguous connectivity; only that case keeps the declared fixed NW–SE split into two triangular contour parents. Binary triangles beside inland-category mixtures likewise retain their triangular parent.

At a crossing shared by two contour parents, consistently oriented incoming and outgoing unit chords define a common tangent direction. An open contour end uses its own chord. For endpoints `P0,P3`, each parent contains a cubic Bézier curve:

```text
B(t) = (1−t)³ P0 + 3(1−t)²t P1 + 3(1−t)t² P2 + t³ P3
P1 = P0 + h0 T0; P2 = P3 − h1 T1; 0≤t≤1
shared tangent T = normalize(unit incoming chord + unit outgoing chord)
```

Handle lengths are capped by one third of the endpoint chord and by the convex parent's half-planes. Their projections occupy the first and last thirds of that chord, respectively. Convex-hull containment keeps the entire curve within its source parent; monotone chord projection prevents a self-loop. Shared tangent directions produce geometrically tangent (`G1`) joins in source coordinates, rather than the former sharp staircase. Handle magnitudes need not match, so parameter derivatives are not claimed to be `C1`. The finite sampled curve retains numerical and tessellation limits. Source post positions and categories remain fixed; no source height is used.

De Casteljau subdivision continues until each subcurve's two interior controls are at most **0.0005 source-grid units** from its closed endpoint segment. Distance to a convex segment is convex, so this bounds the whole subcurve's deviation from that segment in source parameters. One source-grid step is one arc second here; this is a fine tessellation tolerance, **not** centimetre physical coastline accuracy or a certified ECEF error bound. Ear clipping triangulates the two resulting possibly nonconvex class polygons while retaining every sampled boundary vertex. Both materials share the identical ordered boundary vertices.

This is a declared smooth reconstruction from categorical samples, **not a surveyed shoreline or new subpixel information**. Native-scale scalloping, missing small features and source-position uncertainty remain. The finer mesh removes artificial angular teeth but cannot recover an exact beach from approximately 30 m categorical posts.

At refined/coarse boundaries, both sides use the identical ordered native/crossing samples. Transition fans use source-parameter barycenters, not Cartesian barycenters or elevation samples. Every vertex parameter maps to latitude/longitude and then to WGS84 ECEF at h=0. A curved geodetic surface and a Cartesian triangle chord are different: only the former is authoritative for physical queries.

Cells without ocean boundaries retain the conservative 16-post stencil. Both coarse triangles receive a category only when every native post agrees; otherwise class4 means unresolved. Narrow inland channels can therefore be absent at the base sampling scale. This is regional categorical coverage, not a global coastline or a history of shoreline movement. Astronomy time does not change the source epoch.

| Region | Ocean faces | Lake faces | River faces | Unresolved faces |
| --- | ---: | ---: | ---: | ---: |
| region-01 | 0 | 0 | 0 | 20 |
| region-02 | 59,304 | 6 | 270 | 1,951 |
| region-03 | 0 | 4 | 110 | 2,170 |
| region-04 | 237,483 | 929 | 154 | 3,492 |

## Height-free format and categorical query

`AEBNDR01` has a 56-byte little-endian header: magic, u32 base width/height, f64 west/north/coarse longitude-step/latitude-step, then u32 vertex/face counts. Each vertex stores only two f64 values, **local [row,column] in native WBM grid units**, with north-west origin and base spacing three. Each face stores three u32 indices and one category byte. There is no elevation field or hidden Cartesian height. Per-file provenance records source hashes, registration, curve parents/controls/subdivision and topology recipes.

For local parameters `(r,c)`:

```text
latitude  = north − r × coarse_latitude_step / 3
longitude = west  + c × coarse_longitude_step / 3
height    = 0
```

The exact extrema use the same stored bounds arithmetic as the surrounding display mesh. The parser rejects malformed lengths, unsupported geographic grids, nonfinite/out-of-range parameters, changed retained-grid coordinates, invalid class/index values, reversed/degenerate faces and faces crossing their registered coarse-cell boundary. Independent source validation additionally checks full manifold coverage and curve/class consistency.

After an analytic ellipsoid hit, its latitude/longitude converts back to source parameters. Oriented edge signs locate its class face in local coarse-cell bins. Neighbor bins and a 1e-8-native-unit edge tolerance handle floating-point roundoff; that tolerance is not a physical uncertainty. Shared-edge decisions conservatively prefer a non-grass class. Class4 and positions outside coverage report **category not mapped**; no ocean or land is inferred. Exact boundary hits can carry either adjacent material category within this numerical tolerance without changing physical position or range.

## Physical surface versus presentation

`TerrainScene::boundary_only` accepts only height-free patches. Ground placement returns the exact input latitude/longitude at h=0. Earth range uses the analytic WGS84 quadratic; category lookup occurs afterward. Collision and heading-specific skyline use that same ellipsoid, and L/V guides and F9 clearance ignore category display triangles. Entered negative feet heights are rejected. Decorative grass roots are evaluated on the ellipsoid only where the declared category is land.

Rendering evaluates all category vertices at h=0, uses analytic ellipsoid normals, and shares regional perimeter subdivisions with the surrounding global mesh. These finite display chords and f32 rasterization are approximations to the smooth surface. They cannot change the f64 Earth range or physical horizon. The water material is opaque illustrative shading; normal ripples do not move vertices. There is no swimming, seabed collision, tide, wave displacement or measured vegetation. See [rendering.md](rendering.md).

The optional geoid display remains independent: `H_EGM2008=h−N`. Because all modeled ground/water has h=0, its displayed height above that datum is `−N`, generally nonzero. This intentional ellipsoid surface is **not** geoid sea level or the actual ocean. The [NOAA geoid explanation](https://oceanservice.noaa.gov/facts/geoid.html) distinguishes the gravity reference; real water additionally varies with tides, currents and weather. No geoid grid is used by the boundary importer.

## Reproduction and independent review

Install [pinned optional requirements](../../scripts/requirements-terrain.txt), then fetch only the reviewed WBM sources. The existing cache environment can be used as follows:

```powershell
python scripts/fetch_water_sources.py
$env:PYTHONPATH = (Join-Path (Get-Location) '.cache/python')
python scripts/prepare_boundaries.py
python scripts/prepare_boundaries.py --check
python scripts/verify_boundaries.py --report .cache/boundary-verification.json
python -m unittest discover -s scripts -p test_boundary_geometry.py
```

The verifier independently reads WBM files and checks hashes, point registration, retained/native coordinates, parameter barycenters, categories, parent containment, tangent joins, flatness and shared curve sampling. Internal edges must pair oppositely, the patch has one perimeter, and source-parameter faces cover the registered rectangle without holes/overlap. It does not import preparation code or read DSM/geoid data. Rust checks compare analytic queries across every class and coverage edge, validate h=0 vertices and display seam pairing, test negative-height rejection and confirm posture, motion, skyline, guides and F9 are category-independent. See [verification.md](verification.md) for actual runs. These checks validate the declared model, not 100% real-world coastline accuracy.
