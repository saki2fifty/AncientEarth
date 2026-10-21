# Cartographic imagery and registration

> **Historical pipeline, not the active surface.** Current runtime uses a smooth WGS84 ellipsoid at h=0 everywhere and WBM-only category borders. DSM elevations, geoid surface displacement and photographs described below are archived and not loaded. See [current model](model.md) and [boundary preparation](water.md).

This document records the **archived photographic renderer**. The current application uses procedural materials and no photographic terrain imagery. These files, scripts and registration tests remain in the repository for provenance and reproducibility; their presence does not mean the current executable uses them.

## Archived global imagery

The archived renderer used **NASA Blue Marble: Next Generation, July 2004 base map**, credited to NASA Earth Observatory. The [source page](https://science.nasa.gov/earth/earth-observatory/blue-marble-next-generation/base-map/) provides the full-globe image. `scripts/prepare_blue_marble.py` checks the documented input dimensions, preserves the complete geographic extent and prepares a 10800 × 5400 JPEG with Pillow LANCZOS. The chosen resampling and registration were reviewed by the math agent.

`assets/textures/blue-marble.provenance.json` records the exact source URL, input/output hashes, retrieval date and processing. The imagery is a historical monthly composite. It cannot establish current land cover or supply individual buildings.

```powershell
python scripts/prepare_blue_marble.py .cache/terrain/blue-marble-source.jpg --retrieved 2026-10-20
```

Use the actual download date when preparing a new snapshot.

## Japanese aerial imagery

The Tokyo and Mount Fuji terrain patches use **GSI seamless aerial photography**, zoom 14, from the [GSI tile catalog](https://maps.gsi.go.jp/development/ichiran.html#seamlessphoto). Each mosaic retains the native tiles' Web Mercator projection. It is not sampled as a linear latitude/longitude image. The full tile outer-edge bounds are recorded separately from the terrain grid's sample-center bounds.

The math-owned `scripts/imagery_coordinates.py` computes tile selection, outer geographic bounds, mosaic dimensions and tile placement. `scripts/fetch_aerial.py` downloads the selected public tiles, validates them, stitches them without reprojection or resizing, and records every tile URL and hash. The runtime math library maps the terrain hit into the same Web Mercator image.

Following the [GSI XYZ tile convention](https://maps.gsi.go.jp/development/siyou.html), let `n = 2^zoom`, longitude `λ` be in degrees and latitude `φ` in radians:

```text
tile_x = n(λ+180)/360
tile_y = n[1−asinh(tanφ)/π]/2
φ(tile_y) = atan(sinh(π[1−2 tile_y/n]))
```

Tile selection uses floor on the north/west and ceiling minus one on the south/east, making exact outer south/east bounds exclusive. Full mosaic bounds use integer tile edges, not centers. The Web Mercator latitude domain is limited to `±atan(sinhπ) ≈ ±85.05113°`. Spherical Mercator is the imagery convention; the terrain itself remains ellipsoidal ECEF geometry.

## Texas NAIP imagery

Houston and Galveston use public-domain **USDA National Agriculture Imagery Program (NAIP)** data from the [official USGS image service](https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer). The bundled images use the service's 2022 Texas source footprints. The fetch script records their object IDs, acquisition dates, declared source resolution and original datum, then locks the export to those IDs. No commercial basemap layer supplies this imagery.

The mathematical export plan uses spherical Web Mercator with `R = 6,378,137 m` and angles in radians:

```text
X = Rλ; Y = R asinh(tanφ)
λ = X/R; φ = atan(sinh(Y/R))
```

The requested pixel grid has at most 4,000 pixels per axis and approximately equal projected pixel spacing. The [export service may adjust the extent for image aspect](https://developers.arcgis.com/rest/services-reference/enterprise/export-image/), so the request rectangle is never assumed to be the returned image registration. `validate_naip_export` checks the actual JSON response's CRS, finite bounds, dimensions, square projected pixels and coverage of the requested terrain rectangle before converting the **returned outer pixel edges** to degrees.

The resulting Houston image is 3987×4000; Galveston is 3991×4000. Output sampling is approximately 5 m on the ground, coarser than the recorded 0.6 m source imagery. Natural-color rendering and bilinear reprojection/resampling are performed by USGS; original returned PNG bytes and alpha are retained. The source catalog identifies NAD83/UTM imagery; the application uses the server's provided EPSG:3857 transformation and makes no independent survey-grade datum/epoch alignment claim.

Galveston's offshore area contains transparent pixels without NAIP coverage. The renderer falls back to global color there; the underlying Copernicus water-surface geometry remains active. The source city point and surrounding pixels have valid opaque aerial imagery in both Texas exports. This is not bathymetry or imagery of current tidal conditions.

```powershell
python scripts/fetch_naip.py .cache/terrain/imagery-plan.json --retrieved 2026-10-20
python -m unittest discover -s scripts -p test_imagery_coordinates.py
```

[Houston provenance](../../assets/textures/houston-aerial.provenance.json) and [Galveston provenance](../../assets/textures/galveston-aerial.provenance.json) preserve export responses, source footprints, output hashes and registration.

## Runtime local color registration and coverage

For all local images with bounds `west,south,east,north`, the coordinates below use latitude and north/south bounds converted to radians inside `Y`; longitude ratios may use degrees:

```text
Y(φ) = asinh(tanφ)
u = (λ−west)/(east−west)
v = [Y(north)−Y(φ)]/[Y(north)−Y(south)]
x = clamp(u·width−0.5, 0, width−1)
y = clamp(v·height−0.5, 0, height−1)
```

The `−0.5` aligns interpolation with pixel centers. Fractional parts `tx,ty` give weights for the four surrounding pixels:

```text
w = [(1−tx)(1−ty), tx(1−ty), (1−tx)ty, tx ty]
α = Σ w_i α_i
C_local = Σ(w_i α_i C_i) / α          # only when α > 0
C_blend = α C_local + (1−α) C_global
```

Here alpha values are normalized coverage. These color formulas precede the renderer's illustrative shading. The implementation rounds interpolated coverage and unpremultiplied color to 8-bit values, then composites using that coverage; coverage rounding to zero selects only global color. This premultiplied weighting excludes arbitrary RGB stored in transparent pixels. Opaque images reduce to ordinary bilinear color interpolation. Image edges replicate the outermost pixel. These are encoded display-color operations, not radiometric light calculations or new source detail. No color/coverage operation changes terrain vertices or range.

```powershell
python scripts/fetch_aerial.py .cache/terrain/imagery-plan.json --retrieved 2026-10-20
```

The imagery plan is produced by `scripts/prepare_terrain.py`. See [terrain preparation](terrain.md) for that workflow. `assets/textures/fuji-aerial.provenance.json` and `tokyo-aerial.provenance.json` preserve the exact registration and processing. Mixed photo acquisition dates must not be confused with the retrieval date.

Attribution: **Created by editing GSI Tiles (Geospatial Information Authority of Japan)**. See [NOTICE.md](../../NOTICE.md) for the source and usage references. Aerial color does not move the geometry or supply independent collision objects.

## Original Natural Earth reference asset

The original image remains in the repository for provenance and comparison; it is no longer the default global texture.

`assets/textures/earth.jpg` is a 4096 × 2048 derivative of **Natural Earth I, 1:50 million, shaded relief and water**, version 3.2.0 according to the [publisher's dataset page](https://www.naturalearthdata.com/downloads/50m-raster-data/50m-natural-earth-1/). Natural Earth publishes this data in the [public domain](https://www.naturalearthdata.com/about/terms-of-use/).

The source is a global, unrotated, equirectangular TIFF. The source world file supplies pixel-center coordinates and pixel spacing. `scripts/prepare_texture.py` checks that its outer bounds are longitude −180° to +180° and latitude −90° to +90° before doing any resampling. It also checks north-up orientation and a 2:1 image ratio.

The script preserves those complete outer bounds, resamples with Pillow LANCZOS and writes JPEG color data. It saves the source archive, TIFF, world-file and output SHA-256 hashes in `assets/textures/earth.provenance.json`.

Both global assets use **geodetic** latitude and longitude from the active surface hit:

```text
u = ((longitude + 180°) mod 360°) / 360°
v = (90° − latitude) / 180°
```

Longitude wraps at the date line. Latitude clamps at the pole rows. Global texture sampling uses the texel that contains the geographic position; this nearest-neighbor global color lookup does not move the underlying surface point. Local aerial mosaics use the bilinear rule above.

The image is an illustration of land cover and relief. Its finite source scale, resampling, compression, grid overlay and display raster impose visual limits. None changes the exact-model ray used by the dot. The image is not a DEM and cannot support terrain heights, building collisions, or unlimited telescope detail.

To regenerate from the downloaded source:

```powershell
python -m pip install Pillow
python scripts/prepare_texture.py .cache/NE1_50M_SR_W.zip
```
