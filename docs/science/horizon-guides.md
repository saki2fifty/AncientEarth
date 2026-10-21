# Projected Earth skyline guides

**Current model:** every runtime Earth intersection is analytic WGS84 h=0. Regional category triangles cannot raise, mask or change the horizon. Terrain-specific candidate/search discussions below are retained for archived regression fixtures.

**L** places a horizontal guide at the highest point found on the visible canonical Earth outline. **V**, while L is enabled, measures the screen-down gap from that guide to the selected column's uppermost detected sky-to-Earth boundary. Both use the actual eye position, camera orientation including roll, lens and viewport. They do not change where the camera points. Left / Right arrows or the marker drag select V's column.

The active outline is the analytic WGS84 ellipsoid. It excludes decorative grass, the Sun, Moon, stars and GUI panels, and does not depend on the artistic scene brightness. With atmospheric refraction disabled its rays are straight; with a validated active profile they follow the same f64 curved-ray equations and profile as the sight measurement. The separate **Skyline ahead** readout uses a heading-plane calculation described in [visibility.md](visibility.md); it is not the same search.

Weather data can remain loaded while the optical display is unavailable. Outside the current 0–20 km geodetic eye-height range, the app explicitly selects geometric rays for the view, sight measurement and L/V together. A fallback notice remains visible in clean mode. The retained profile does not silently make the guides refracted over a geometric picture. This is an implementation limit, not a claim that atmospheric refraction ends at that height; see [rendering limits](rendering.md#weather-informed-curved-ray-presentation).

## What the lengths mean

The horizontal line spans the viewport, but its two reported lengths describe the **leftmost and rightmost sampled visible outline points**, `Q_left` and `Q_right`. These points need not reach the viewport edges. Disconnected outline pieces are not asserted to form one continuous visible arc.

```text
L chord = |Q_right − Q_left|
L surface = shortest WGS84 ellipsoid geodesic between their latitudes/longitudes
```

The chord is a straight three-dimensional ECEF distance between the selected surface hits. The surface distance projects both endpoints to ellipsoid height zero and uses the robust Karney inverse geodesic algorithm, including near-antipodal configurations. It is a reference-Earth surface distance, not a terrain-following route or a length traced along the visible skyline. Terrain elevation, buildings and grass do not add path length. See [the WGS84 model](model.md) for the axes and flattening and [the geodesic implementation and sources](geodesics.md) for the inverse construction.

**V is labeled “drop below guide … at skyline depth / view-plane gap.”** A screen segment alone does not specify a physical length: its rays can pass through many depths, and the guide endpoint can point into empty sky. The app therefore states its depth reference explicitly.

Let `O` be the eyes, `F` the camera's unit forward axis, `Q` the measured skyline hit at the selected screen column, and `D` the unit ray through the guide at that same column. Construct the plane through Q perpendicular to F:

```text
depth = (Q − O) · F
P = O + D [depth / (D · F)]
(P − Q) · F = 0
V gap = |P − Q|
```

P is a **constructed reference point**, not a measured hit in the sky. P and Q project to the vertical screen segment's endpoints; their distance is measured in the specified camera-facing plane. This remains well-defined for the equidistant fisheye wherever both endpoint rays exist and have positive forward depth. It is not a geodetic height, gravitational vertical drop, surface distance, or an isolated measurement of Earth's curvature. Pitch and roll affect its reference plane and the guide location. No V value is displayed when the selected column lacks a usable sky-to-Earth boundary, the top endpoint lies outside the fisheye aperture, or the forward-depth construction is singular or behind the camera.

When refraction is active, Q is the curved ground-ray hit and P is the first valid forward intersection of the **curved top ray** with that same camera-facing plane. The straight formula for P above is then replaced by numerical integration and plane-event refinement; a vacuum continuation after atmospheric exit is permitted. The reported gap remains the Euclidean distance `|P−Q|` in that plane, not optical path length or gravitational height. If the top ray hits the ground before reaching the plane, fails its integration budget or has no usable crossing, V is unavailable. Multiple/turning optical paths are not resolved by pretending a straight sky ray meets the plane.

## Outline search and precision

All queries use the canonical f64 scene intersection and nearest-hit occlusion, not the display LOD mesh. The finite search starts on a **64 by 36 interval** screen lattice. A pair of valid rays with different hit/miss states brackets an outline crossing. Invalid rays outside a circular fisheye aperture are not treated as sky; aperture edges alone never create an Earth horizon.

The highest eight projected canonical terrain posts provide additional summit seeds. Local extrema are refined in twelve shrinking screen-space steps. Each detected crossing is bisected at most forty times until its local hit/miss bracket is at most **0.025 physical pixel**. V separately searches 128 vertical intervals at its current column, supplemented by the apex row and offsets of ±0.025 and ±0.1 pixel to help retain thin terrain bands. It selects the first detected sky-to-Earth transition, with the same bisection rule. A column that reveals a higher outline than L's estimated apex makes V unavailable rather than producing a reversed measurement. If the difference comes from independent subpixel brackets, an actual hit at the guide row may instead represent V's endpoint, but only when the expanded sky-to-hit bracket remains at most 0.025 pixel.

This is a **sampled silhouette estimate**, not a certified global maximum. Narrow isolated features or gaps between sampled rays can be missed; adding summit seeds does not prove completeness for every occlusion or lens. The local subpixel bracket does not bound the global apex error, endpoint range error, source elevation uncertainty or displayed mesh error. Tangencies are particularly ill-conditioned: a small angular change can produce a much larger change in the hit distance. The reported lengths are computed between the selected finite-search points; formatted decimals are not a claim of matching real-world precision.

Results are cached until the observer, lens, viewport, active surface or weather-profile generation changes. Within one search, identical physical pixel coordinates reuse their hit/miss result, retaining the distinction between sky, an invalid aperture ray and an unresolved optical query. An unresolved ray is excluded; it never supplies a false sky-to-ground bracket. Straight-mode intermediate crossings need only Earth occupancy and defer nearest-hit resolution to final endpoints. Active refraction instead integrates the profile for each distinct sampled direction. The sampling density, tie ordering and local brackets are unchanged.

Moving V recomputes its selected column; changing units only reformats the lengths. Turning L off avoids this additional contour search. Recalculation is synchronous so a displayed measurement is never silently retained from an older camera pose. Curved-ray contour refinement can noticeably reduce frame rate during motion, and profiles that approach ducting can exhaust the finite path budget. The frozen-view cache avoids that cost while stationary; smooth rendering throughput is not a guarantee of equally fast moving-contour measurement. Native checks and their configuration limits appear in [verification.md](verification.md).

## Physical pixels and display

For logical display dimensions `(w,h)` and framebuffer scales `(s_x,s_y)`, the physical viewport is `(W,H)=(w s_x,h s_y)`. Screen coordinates map to rays using

```text
NDC x = 2 x/W − 1
NDC y = 1 − 2 y/H
aspect = W/H
```

The smallest detected outline y is the guide's subpixel reference. L fills physical row `floor(y)`, clipped to the framebuffer; logical bounds divide by the corresponding scale. Non-antialiased filled rectangles give a one-physical-pixel line, including on HiDPI displays. V likewise uses a one-physical-pixel column. Labels use the unsnapped measurement coordinates; the displayed row is less than one physical pixel from that reference. Fisheye's black aperture exterior can contain the full-width guide but contributes no measured outline points.

GPU rendering has finite tessellation, f32 precision and antialiasing. Thus the guide's canonical outline can differ slightly from a visible display triangle or antialiased edge. Source-data coverage, terrain sampling and reference-ellipsoid limitations remain those documented in [terrain.md](terrain.md) and [rendering.md](rendering.md).

Implementation: [`guides.rs`](../../crates/earth-math/src/guides.rs). Independent verification is recorded in [verification.md](verification.md).
