# Sea-level reference and vertical measurements

The current surface is WGS84 h=0 everywhere. EGM2008 is retained only as an independent HUD datum readout: surface height above this reference is −N. It does not displace land/water or enter boundary preparation.

AncientEarth reports the **EGM2008 geoid beneath the observer**, evaluated at the observer's current geodetic latitude and longitude. This is a modeled sea-level reference, including over land. It is not the instantaneous ocean surface, a tide prediction, a surveyed local vertical datum, or an exact measurement of physical sea level.

NGA publishes EGM2008 and its worldwide 2.5-arc-minute geoid grid. The bundled source is the [PROJ-distributed NGA GeoTIFF](https://cdn.proj.org/us_nga_egm08_25.tif), whose [provider record identifies it as public domain](https://github.com/OSGeo/PROJ-data/blob/master/us_nga/us_nga_README.txt). See [NGA's model description](https://earth-info.nga.mil/index.php?action=wgs84&dir=wgs84) for the gravity model. Source hashes, registration, software versions, validation results and output hashes are in [`egm2008-native.provenance.json`](../../assets/geoid/egm2008-native.provenance.json).

## Native grid and interpolation

All **8,640 × 4,321 native float32 samples** are retained without resampling or precision reduction. Source sample centers, rather than raster outer edges, determine registration:

```text
longitude(column) = −180° + (column / 24)°
latitude(row)     =   90° − (row / 24)°
```

There are 8,640 distinct longitudes; the east neighbor of the last column is column zero. Both polar rows are present and constant in longitude. The importer verifies `PixelIsPoint` metadata, source CRS, registration, missing values and polar consistency against the [PROJ geodetic-grid specification](https://proj.org/en/stable/specifications/geodetictiffgrids.html). No additional half-pixel shift is applied.

For longitude λ and latitude φ in degrees, the Rust query computes:

```text
x = ((λ + 180°) modulo 360°) × 8640 / 360°
y = (90° − φ) × 4320 / 180°
i = floor(x), j = floor(y)
u = x − i, v = y − j
N = (1−u)(1−v) N[j,i] + u(1−v) N[j,i+1]
  + (1−u)v N[j+1,i] + uv N[j+1,i+1]
```

Column indices wrap; the south-pole row is clamped. Grid values are promoted to float64 before interpolation. `N` is geoid undulation in metres above the WGS84 ellipsoid. Bilinear interpolation approximates the field between the finite-resolution source posts; retaining all source bits does not make the continuous gravity model or physical sea level exact.

## Heights and the ground beneath a flying observer

The observer's feet, eyes, ground query and geoid query use the same latitude and longitude. Ground is found by intersecting the active surface along the geodetic normal. The [standard geoid-height conversion](https://geographiclib.sourceforge.io/html/GeoidEval.1.html) is:

```text
H = h − N
h_eye = h_feet + effective_eye_offset
ground_above_geoid = h_ground − N
eyes_above_geoid   = h_eye − N
feet_above_ground  = h_feet − h_ground
eyes_above_ground  = h_eye − h_ground
```

Here `h` is ellipsoidal height and `H` is the corresponding EGM2008-referenced height. The configured eye offset is measured above the feet; eyes above ground also include any flying clearance. These clearances follow local **geodetic up**. They are not the shortest distance to a slope, a gravity-plumb-line integration, or the dot's oblique sight range.

Negative ellipsoidal heights remain valid. For example, the geoid near Houston and Galveston is below the WGS84 ellipsoid. A negative `h` therefore does not by itself mean below the modeled sea-level reference. Outside imported terrain, ground falls back to `h = 0`; its reported EGM2008 height is consequently `−N`, not necessarily zero. The geoid readout does not replace the fallback geometry with an ocean surface.

## Posture and eye position

Standing eye height defaults to **1.72 m**, crouching eye height to **1.00 m**, within a six-international-foot stature of **1.8288 m**. These two eye offsets are configurable application assumptions, not anatomical measurements. Crouching uses the smaller of the configured crouching and standing values. Both settings are restricted to 0.1 m through the stature.

Changing posture keeps the feet fixed and changes the eye ECEF position, all camera rays, dot range and skyline query. The maximum feet altitude reserves the standing offset even while crouched, so releasing C does not require moving the feet or exceeding the supported observer ceiling. At an invalid prospective setting or destination, validation rejects the change.

## Packaging and reproducibility

[`prepare_geoid.py`](../../scripts/prepare_geoid.py) writes `assets/geoid/egm2008-native.aeg.gz`. Install the pinned optional dependencies in [`requirements-terrain.txt`](../../scripts/requirements-terrain.txt), retain the source GeoTIFF at `.cache/terrain/us_nga_egm08_25.tif`, then run:

```powershell
python scripts/prepare_geoid.py
```

The inflated `AGEGEO01` container has a 48-byte little-endian header: eight-byte magic, unsigned 32-bit width/height and four float64 registration values. Each row stores the four little-endian float32 byte planes consecutively, then differences successive bytes modulo 256, restarting from zero at each row. Decoding uses a modulo-256 prefix sum and reverses the byte shuffle. Gzip uses a deterministic zero timestamp.

This lossless codec reduces the bundled file to **81,581,387 bytes**; decoded samples occupy **149,333,760 bytes**. Preparation decodes and compares every source bit. Seventeen independently evaluated PROJ locations cover multiple continents, the longitude seam and both poles; maximum interpolation difference is `6.39 × 10⁻¹⁴ m`. These comparisons establish processing agreement, not physical uncertainty. Runtime parser/interpolation and posture tests are described in [verification.md](verification.md).
