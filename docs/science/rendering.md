# GPU presentation mathematics and limits

With atmospheric refraction disabled, the native view uses Bevy 0.20's PBR raster pipeline at the physical framebuffer resolution, with four-sample MSAA. With a validated active weather profile, a separate curved-ray pass traces the analytic ellipsoid at native pixel resolution, as described below. The default window is 1920×1080. There is no low-resolution Earth image enlarged to fill the screen, and no photographic imagery is loaded.

The authoritative physical surface is the analytic f64 WGS84 ellipsoid at ellipsoidal height `h=0`, everywhere on both land and water. Source-derived regional boundaries select materials and suppress grass; they do not supply elevations. No DEM or geoid heights shape the current scene. The GPU has finite tessellation, f32 arithmetic and shadow-map resolution. Numerical readout precision must not be interpreted as a guarantee that every displayed chord exactly coincides with the curved scientific surface.

## Coordinate precision

Let the floating origin be `O` in ECEF metres, with local geodetic east `E`, north `N` and up `U`. Before conversion to f32, a geographic vertex `P` is transformed as

```text
X = dot(P − O, E)
Y = dot(P − O, U)
Z = −dot(P − O, N)
```

East/up/south is right handed because `E × U = −N`. Camera directions use the same basis without subtraction. The camera's actual eyes include the selected standing/crouching offset. Floating origins are rebuilt after a position-dependent movement threshold between 80 m and 20 km; eye-relative subtraction still occurs in f64. Nearby f32 coordinate spacing is therefore much smaller than the roughly half-metre spacing of raw Earth-radius f32 positions. Far-side planet vertices and astronomical body positions remain less precise than local terrain; this is a presentation boundary, not a modification of their f64 source values.

## Surface tessellation and LOD

Regional boundary meshes retain the source-derived land/water partition at every viewing distance. Every vertex is generated at WGS84 `h=0`, including reconstructed shoreline samples. The renderer applies the analytic ellipsoid normal to both material copies of a boundary vertex. There is no height- or slope-based land color. The unclassified global fallback uses the identical illustrative land material; that appearance is not evidence of land cover outside verified regional masks.

The application builds the global and regional meshes together. Along each regional coverage perimeter it takes the union of every existing boundary vertex and every adaptive reference-cell endpoint. Both sides use those same geographic posts and the same `h=0` conversion. Regional perimeter triangles with inserted edge posts are split into a fan about their geographic centroid; each child retains the original material. Ordered edge subdivisions and the centroid partition the original convex parameter-space triangle. This removes regional/global T-junction gaps without an elevation offset or overlapping skirt. Interior coastline triangles are unchanged by this display stitching.

The screen selection heuristic is

```text
focal_pixels = framebuffer_height / (2 tan(vertical_FOV / 2))
projected_error_estimate = geometric_bound × focal_pixels / minimum_range
```

The minimum range uses a containing sphere. This is a heuristic in pixels, not a strict off-axis perspective error proof. Very wide fields of view magnify edges differently from the image center.

Reference-ellipsoid cells use geographic latitude/longitude subdivisions. With `a`, `b`, and `e²` from WGS84, let

```text
R = a² / b
q = 1.5 e² / (1 − e²)
K = R sqrt(1 + q²)
angular_span = Δlatitude + Δlongitude             # radians
geometric_chord_bound = K × angular_span² / 8
```

`R` bounds the principal curvature radii. `K` bounds the second derivatives of the ellipsoid's geodetic parameterization; the bound accounts for the changing meridional radius. A Taylor/barycentric interpolation bound supplies the stated chord bound. Each leaf is fan-triangulated through all neighboring edge posts, avoiding T-junction cracks inside the reference mesh.

The four regional source rectangles partition the reference mesh. Their material coverage ends at the documented mask boundary, but their physical height does not change there. Scientific range, collision, ground and skyline queries use the analytic ellipsoid rather than either mesh's chords. Finite triangle sag, f32 precision and the display budget remain presentation errors; they are not mountains, ocean depths or scientific elevation values.

The GUI LOD factors are 0.5, 1, 2 and 4, applied to the selection focal length. The effective focal length for mesh selection is capped at 16,000 pixels and reference refinement at 17 subdivision levels. Regional classification meshes retain their complete finite partition rather than simplifying across a coastline. Optical field of view itself is not capped by this mesh budget. These finite limits prevent telescope settings from allocating billions of triangles; high magnification can reveal the finite mesh. No unlimited-resolution claim is made. Legacy sampled-terrain helpers remain testable in the math library but are not the application's active surface model.

## Rectilinear and equidistant optics

The normal and telescope views use Bevy's vertical-field-of-view perspective matrix. Camera near clipping is 0.02 m; far culling is 4×10¹² m. This encompasses the supported Earth–Moon–Sun scene but is finite.

Fisheye renders six 90° square perspective views at the framebuffer height, with directions `+F`, `+R`, `−F`, `−R`, `+V`, `−V`, where `F`, `R`, `V` are camera forward, right and up. Up/down face up vectors are `−F` and `+F`. The final original shader computes the ideal equidistant direction:

```text
x = aspect × (2u − 1)
y = 1 − 2v
ρ = sqrt(x² + y²)
θ = ρ × vertical_FOV / 2
direction = (x sinθ/ρ, y sinθ/ρ, cosθ)
```

At the center the direction is forward. Pixels with `ρ > 1` are outside the circular aperture. The dominant direction component selects a cube face, and dividing the other two components by that dominant component recovers its perspective UV coordinates. Bilinear texture sampling then performs the finite reprojection. It requires multiple GPU scene views and costs more than the normal perspective. Face sampling is finite; narrow fisheye fields of view do not have unlimited central image detail.

## Grass and range

The blades are original illustrative geometry, not surveyed vegetation or a land-cover classification. A jittered global latitude-row lattice with longitude spacing derived from each row fixes placement to Earth; longitude indices wrap at the date line. Every root is independently projected onto the analytic `h=0` surface. Only mapped, unmixed land admits roots: water, mixed boundaries and unclassified locations are rejected before entering the placement cache. Both primary and basal roots receive this check. The source water mask does not establish that eligible land is actually grassland. Primary blades stand 7.5–19 cm tall; a deterministic hash retains approximately a quarter of these taller stems. Three more strongly leaning leaves per eligible cell stand 4.5–9 cm tall, have their own nearby roots and fade at individual distances of 4–6 m. This puts most geometry into low turf near the observer. Curved blades and their material do not change the ellipsoid or flight collision.

Each blade has nine vertices and seven triangles, with a tapered width, curved resting shape, individually varied color and bounded wind. Quality sets maximum visibility distances of 12, 18, 22 or 26 m; individual cutoff distances vary to avoid a common visible ring. Over the last 3 m before a blade's cutoff, cubic smoothstep shrinks its geometry about its anchored root. The fade uses the actual observer's eye position for both the forward and shadow passes. A 5 m generation guard allows camera motion between static mesh uploads.

The CPU uploads a resting position and four displacement coefficient vectors per vertex. Every frame it supplies four bounded sine/cosine weights. The GPU evaluates the same linear combination in both its color and shadow vertex shaders. Base coefficients vanish, so wind does not detach roots. A conservative culling box includes the root and resting position plus/minus the sum of absolute coefficient components, with outward floating-point padding. This contains every wind state and root-relative fade state. Static mesh uploads occur only when generation, origin or quality changes; wind updates do not rebuild the whole mesh on the CPU. Normals follow the curved resting blade and are an approximation during deformation.

Canonical root queries are cached by fixed spatial cell, and the sight query uses cached conservative blade bounds before exact triangle tests. A background task prepares the complete resting mesh, blade list and bounds. A completed snapshot is accepted only for the current immutable terrain scene, generation and an observer within 4 m of its generation point. Scene, origin, quality and enable changes invalidate pending work. Normal movement keeps the previous coherent visible/range snapshot while a replacement is prepared; scene or origin changes clear both together. Fast movement can temporarily reach beyond that field's prepared coverage. Briefly absent decorative grass during preparation does not alter terrain geometry or measurements. Snapshot readiness is exposed to screenshot and frame-pacing checks.

The sight query uses those same quantized local attributes, current f32 wind weights and fade definition. A double-sided ray/triangle intersection returns the closest blade only when it is nearer than the authoritative terrain. GPU fused arithmetic and rasterization may differ by small rounding amounts from the CPU query. The HUD identifies decorative grass separately. This measures generated geometry, not a claim that real grass occupies that location. Thin-leaf transmission, wind and material colors are illustrative, not measured vegetation or weather. A daylight-gated diffuse sky-fill term softens dark back-facing leaves and ground shadows. It is modulated by material color, multiplied by camera exposure once, and added to the lit output before fog and tone mapping. The ground term also has a bounded upward-hemisphere weight. These artistic shading terms do not alter geometry, shadow silhouettes or sight intersections and vanish at night or when the astronomy frame is unavailable.

## Classified water presentation

Water is displayed only inside regions classified from the bundled source mask. Source preprocessing reconstructs supported binary native-cell land/ocean boundaries using outer-edge crossings and bounded G1 curve samples; [water reconstruction](water.md) defines the retained boundary model and its source limits. The height-free boundary asset stores geographic parameters and material classes. Runtime vertices are generated on the WGS84 ellipsoid and partitioned into disjoint land and water meshes. The renderer applies no independent coastline smoothing and places no overlapping flat water plane. Any boundary still classified as mixed retains land material and suppresses grass. Native source resolution and the declared reconstruction model still limit shoreline position; added vertices do not constitute additional surveyed detail. Measurement and collision hit the analytic ellipsoid, then classify the hit in the same geographic boundary partition, independently of finite GPU triangles.

Ocean, lake, river and land all share `h=0`. This is the requested idealized reference ellipsoid, not mean sea level, a geoid, an instantaneous tide, water depth or bathymetry. Outside bundled regional coverage there is no inferred global water classification. No official topographic elevations are loaded into this surface model.

The original opaque water material uses Bevy PBR, ordinary terrain shadow maps, four-sample MSAA and the same local eclipse field as land. Its color and chosen dielectric normal-incidence reflectance `F0=0.02` are appearance parameters, not measurements of local water optics. Bevy maps its reflectance control `r` through `F0=0.16 r²`, so the supplied control is `sqrt(0.02/0.16)`. A perceptual roughness of 0.22 keeps highlights finite. There is no photographic texture, bottom visibility or refracted underwater scene.

Animated ripples alter only the shading normal. They do not move water vertices, change the horizon or alter ray ranges. Three original periodic slope fields use integer ECEF direction vectors `(1,2,0)`, `(-2,1,1)` and `(1,-1,3)` and wavelength parameters 64, 16 and 4 m. Each divides the 4096 m coordinate period, preserving the pattern through floating-origin rebases. These are illustrative patterns, not a hydrodynamic dispersion model or measured wind. The CPU passes bounded sine/cosine pairs for angular frequencies 0.33 and 0.61 radians/s; angle addition supplies the third phase without an ever-growing f32 timestamp.

Before periodic wrapping, screen derivatives estimate the ECEF footprint. Each slope fades between phase footprints of 0.2 and 0.8 radians per pixel, suppressing distant shimmer without claiming an exact bandlimit. The combined slope is projected into the surface tangent plane and bounded to length 0.07 before normalizing the perturbed normal. There is no high-frequency color noise or geometric wave displacement.

An explicitly artistic sky-reflection approximation reflects the eye direction about this shading normal and uses the sky sphere's original colors. Its horizon brightness factor is `0.8+0.2 smoothstep(−0.08,0.08,R_up)`, where `R_up` is the reflected unit direction's local-up component. This continuous transition avoids a brightness jump as ripples reflect across the horizon, especially at telescope magnification; it is not measured horizon radiance. Schlick's weight `F=F0+(1−F0)(1−N·V)^5` blends its angular strength; the dot product is clamped to `[0,1]`. It does not reflect shoreline objects, clouds or catalog stars and is not a complete energy-conserving water radiative-transfer model. Direct PBR illumination receives local solar visibility once; diffuse fill uses local artistic eclipse response and camera exposure once. The reflected sky gradient is already in the unlit sky's scene-linear display space, so it is not multiplied by camera exposure again. Its eclipse response is evaluated locally rather than applying the observer's eclipse factor twice.

## Procedural ground material

The ground shader adds original turf, soil and stone shading without changing any terrain vertex. Four periodic value-noise bands use cell widths of 16 m, 2 m, 0.125 m and 0.03125 m. They control restrained color variation, roughness and small surface-normal details; these details are not surveyed surface structure or land-cover data.

Material coordinates are reconstructed from the local east/up/south basis and the f64 ECEF floating origin reduced modulo 4096 m. Every cell width divides that period, and integer hash coordinates wrap by the corresponding number of cells. Shifting the floating origin by a period therefore preserves the material pattern, within local f32 precision. This also avoids a separate map projection or a UV seam at the date line.

Each band uses cubic interpolation and its analytic spatial gradient. Screen derivatives are computed before wrapping the coordinate. The band fades toward its mean as its projected footprint grows from 0.06 to 0.22 cell widths, suppressing fine aliasing without claiming an exact mathematical bandlimit. All decorative material variation also fades smoothly between 40 m and 350 m from the camera, leaving a smooth distant surface without artificial speckled geography. The combined shading gradient is projected into the original surface tangent plane and bounded to length 0.35 before the final normal is normalized. It cannot reverse the normal's hemisphere. It changes lighting only: silhouette, analytic ellipsoid shape, ray hits and flight collision retain the underlying surface.

## Celestial geometry, light and stars

The astronomy library supplies apparent ECEF body centers, physical radii and body-to-ECEF rotation matrices. The renderer uses those centers and radii directly in metres; it does not enlarge the Moon or place the Sun at a nearer arbitrary distance. Body orientation is composed with the local coordinate basis. See [astronomy](astronomy.md) for ephemerides, time scales and accuracy limitations.

The Moon is a finite sphere with 192 longitude segments and 96 latitude segments. Its original smooth albedo variation is illustrative, not a lunar photograph or real lunar surface map. A fragment shader computes Lambert lighting from the normalized, rotated surface normal and Moon-to-Sun direction in the same local frame. Their dot product is invariant under the common frame rotation. Only the light-direction uniform changes during playback; its sphere mesh remains static. A small artistic minimum brightness keeps its dark surface distinguishable. The Sun uses the same finite angular tessellation. Sphere meshes are not lunar or solar topography. Current body shading does not reproduce physical eclipse photometry, atmospheric extinction or diffraction.

The 3,000 decorative star points come from an original deterministic generator. Their frame follows the supplied ICRF-to-ECEF rotation, but they are not a scientific star catalog, named constellations, measured magnitudes or finite-distance stellar parallax. Their drawn size follows the viewport's pixel scale. Visibility follows an artistic daylight/altitude function, with an additional total-eclipse rule described below. Unlit celestial colors bypass camera exposure in Bevy's standard material and still pass through tone mapping. These display colors are not calibrated photometry.

Sunlight uses Bevy directional lighting and four cascaded 4096² shadow maps, with cascades extending to 12 km. The first cascade ends at 12 m for nearby grass. Terrain and moving grass cast these shadows; the global reference ellipsoid does not, because an Earth-sized caster destroys the depth precision needed by nearby blades. Its Lambert orientation and local nighttime light policy supply its day/night appearance. Bevy's default depth/normal biases mitigate shadow acne. Terrain bounds update with its geometry; grass uses the explicit wind-bounded culling box described above.

Shadows beyond the configured range, finite-map bias/filter errors and atmospheric scattering are not exact physical simulations. An original smooth sky-color gradient is drawn on a sphere at 3.5×10¹² m, behind the modeled bodies and decorative stars; it is a background, not a physical atmosphere or range target. Its color, warmer low-Sun tint, hemisphere fill and ambient-light level are illustrative clear-sky display choices. Bevy's exponential distance fog gives the terrain restrained aerial perspective; it is not local weather data. Sun, Moon, stars and the sky material explicitly disable this fog. No photographic LUT asset is needed by the selected `SomewhatBoringDisplayTransform` tone mapper.

Optional weather-informed atmospheric refraction uses a separate viewing-ray path. The PBR sky and fog described above remain the straight-ray mode's artistic presentation; neither is itself a refractive model. See [the optical model and weather inputs](refraction.md).

## Weather-informed curved-ray presentation

When a validated profile is active, a fullscreen shader launches the same rectilinear or equidistant lens directions used by the CPU. Telescope magnification changes the ray launch angles directly; this mode does not warp an already rendered straight-ray image or sample the six fisheye images. It integrates the geometric-optics equation through the profile's piecewise-linear phase refractivity `n−1`, with gradient along the local geodetic normal. The physical surface remains exactly WGS84 `h=0`. Decorative grass is hidden in this mode because its finite raster geometry has no matching curved-ray intersector. Switching refraction off restores the PBR/grass path.

The shader keeps positions relative to the actual eyes in east/up/south coordinates. To evaluate geodetic height without subtracting rounded Earth radii, define `M = a Bᵀ Q B`, `g = a Bᵀ Q O`, and `f₀ = a(OᵀQO−1)`, where `Q=diag(1/a²,1/a²,1/b²)` and B has the local basis columns. The CPU forms `f₀` from the known observer height and the exact h=0 footpoint in f64. For local displacement p:

```text
f = f₀ + p · (2g + Mp)
q = g + Mp
h = f / [u·q + sqrt((u·q)² − f(u·Mu))]
u = normalize(q − hMu)
```

Five height/normal iterations evaluate the actual geodetic parallel surface locally. This is not an ellipsoid with axes `(a+h,b+h)`. Profile knots store `n−1` separately, avoiding loss of small refractivity in an f32 number close to one. The CPU's independent f64 inverse and solver remain authoritative for measurements.

The GPU uses midpoint step doubling, with at most 2 km per proposed step, 4,096 attempted steps and 2,000 km of atmospheric path. Its display error control is `max(0.002 m, 4×10⁻⁷ |p|)` for the position/direction step-doubling estimate; this is a local numerical control, not a bound on accumulated path or skyline error. Ground and atmosphere-top crossings are refined by reintegration. Rays leaving the profile continue from their exit position and tangent. Above-profile rays first bracket and refine the true geodetic-height entry surface. A failed integration or exhausted budget within the active optical mode produces a diagnostic purple pixel, not a fabricated surface hit or sky classification. Native f32 arithmetic, cancellation at long distances and grazing tangency limit display agreement with the separate f64 measurement solver.

Step doubling alone can miss a thin gradient layer. Both solvers therefore also bound the next step by the nearest upper and lower profile knots, using the positive root of `gap = v s + K s²/2`, with signed height velocity v toward that knot and conservative height-curvature bound `K = 1/[a(1−e²)] + max|dn/dh|`. The display excludes a small knot band `max(10⁻⁵ m, 4×10⁻⁷ |p|)` to avoid repeatedly approaching the same numerically indistinguishable boundary; adjacent layers narrower than four times that band are unresolved. This is an explicit f32 display limit, not permission to flatten a thin weather layer. The f64 solver has its own smaller numerical band and independent convergence criteria.

The application activates this GPU path only for geodetic eye heights from 0 to 20,000 m. Outside that operational range it explicitly selects the geometric renderer, geometric range and geometric L/V guides together, while retaining the downloaded weather profile and showing a fallback notice even in clean view. The limit is a conservative presentation policy, not a physical boundary of refraction or a guarantee that every ray inside it resolves. An orbital eye-relative origin made the near-ground profile knots numerically indistinguishable in native testing; supporting those views needs atmospheric-entry rebasing rather than a larger advertised limit.

Independent finite coefficient fixtures at four observer latitudes and nearby surface/atmosphere targets found maximum height differences of about 0.071 m for eye heights through 100 km, and about 2.94 m at 10,000 km. These higher-altitude coefficient diagnostics do not extend the operational optical domain. The values include local-position quantization and are fixture results, not universal or accumulated optical-ray error bounds. Near a tangent, even a small height error can move a surface endpoint appreciably.

An independent scalar f32 transcription also traced 18 rays through the retained NOAA profile at 10 km and 20 km eye heights: three headings, directions 0.01° above and below the CPU skyline, and straight down. All resolved and agreed with the f64 ground/sky classification; maximum ground-endpoint and geometric-path differences were about 19.66 m and 35.41 m. These finite grazing cases demonstrate why a high-altitude rendered endpoint is not promised to match the f64 metrology to a metre. They exclude native GPU contraction/transcendental differences and are not global error bounds.

At a ground hit, the shader derives geographic angles from the analytic normal and tests the retained regional parameter-space triangles in a storage buffer. Coarse source cells index the candidate triangles; there is no low-resolution categorical image or independently moved shoreline. Edge tests have finite f32 precision. Outside regional coverage the same unclassified land appearance remains. Curved viewing rays can expose different surface points from straight rays, including turning-ray behavior where supported by the supplied profile and finite integrator.

Sun and Moon intersections use the atmosphere's **exit position and direction** against the existing apparent finite spheres. Using only the exit direction from the original eye would lose the path's finite-body parallax. The supplied ephemeris/apparent-center model is retained: atmospheric optical delay is not fed back into a new coupled retarded-time ephemeris solution. Lunar phase remains a Lambert sphere approximation. An original illustrative angular star pattern follows ICRF orientation; it is not a catalog or a photometric model. Its spot radius is capped using the optical focal scale so telescope zoom does not enlarge decorative dots into large discs. This is approximate pixel sizing because the angular cube parameterization has a direction-dependent Jacobian. Equidistant radial focal scale is `framebuffer_height / vertical_FOV_radians`; rectilinear uses the tangent formula above.

Radiance in this mode is an explicitly simplified original shader: diffuse land, a restrained periodic detail band, opaque water with a finite ripple normal and sky-reflection approximation, an illustrative sky gradient, and finite haze. Earth day/night and the shared pointwise eclipse model still attenuate direct illumination. Decorative grass shadow maps are absent with the hidden grass. Viewing-ray refraction does not claim full refracted solar illumination, atmospheric scattering/extinction, cloud transport, physically measured water optics, or calibrated brightness. This pass uses one optical trace per native pixel; raster MSAA does not antialias its internal ray-defined coastline. Profile quality, finite pixel sampling, f32 integration and source boundary resolution remain independent limitations.

Shading detail fades using the footprint estimate `path / [focal × max(|N·u|,10⁻⁶)]`, where N is the analytic hit normal and u the arriving unit tangent. The grazing factor follows the largest local planar projection stretch and prevents dense ripple stripes at a telescope horizon. A curved optical map can have additional focusing or caustics, so this remains an appearance-filtering heuristic rather than a certified Jacobian bound. It changes neither the ray, hit, horizon, source classification nor measured distance.

## Moving solar-eclipse shadow

The terrain patches, classified water, reference ellipsoid and grass evaluate solar visibility at each rendered fragment. This is a spatial field: the Moon's umbra, antumbra and penumbra move over Earth as the ephemeris time changes. It does not rely on a single observer-level directional-light dimmer or on including the Moon in the nearby terrain shadow maps.

The astronomy library supplies independently retarded Sun and Moon states anchored at the current observer. For other positions, the field corrects the receive-time light paths using each body's emission velocity and applies the same observer-aberration convention as the visible celestial spheres. This is a linear-state spatial approximation; a fresh ephemeris sample remains authoritative at its own observer. See [astronomy](astronomy.md) for the timing model, sphere radii, reference sources and limitations.

Before f32 quantization, the field is rotated in f64 into a Sun-aligned frame. Three affine rows transform a rendered local position into the anchor-relative shadow coordinates. This preserves the small lunar transverse offsets relevant to eclipse alignment, instead of subtracting large almost-parallel f32 vectors in arbitrary Earth axes. Sun/Moon vectors, physical radii, velocities and observer velocity are packed in the same frame. The GPU uses the same six light-time iterations as the CPU field, evaluating a difference of distances through a difference-of-squares identity to reduce cancellation.

For a surface point, the sphere angular radii are `α = asin(Rsun / dsun)` and `β = asin(Rmoon / dmoon)`. Directional separation is `δ = atan2(|usun × umoon|, usun · umoon)`, avoiding the loss of precision in `acos` near alignment. A Moon farther away than the Sun does not obscure it. The circular angular-plane overlap uses normalized separation `d = δ/α` and lunar radius `r = β/α`. Disjoint and contained cases are handled explicitly. Partial overlap uses the chord coordinate

```text
a = 0.5 × (d + (1 − r)(1 + r)/d)
h = sqrt(max(0, (1+r−d)(d−(1−r))(d+(1−r))(d+1+r))) / (2d)
A = atan2(h,a) + r² atan2(h,d−a) − d h
V = clamp(1 − A/π, 0, 1)
```

This is the unobscured fraction of a uniform-radiance angular disk. It is a small-angle circular-disk approximation, not spherical-cap solid-angle photometry. It omits solar limb darkening, lunar limb topography, diffraction, corona and atmospheric radiative transfer. Annular eclipses retain a nonzero direct fraction; total eclipses reduce direct sunlight to zero.

The current ground, grass and water materials are non-emissive and receive only sunlight plus solar-derived ambient illumination. Their exposed PBR output, including the existing ordinary shadow-map result, is multiplied by the local `V`. Thus the approximation also scales the built-in solar ambient and transmitted/specular sunlight; independently powered lights would require separating those contributions. The separate artistic diffuse fill uses `0.04 + 0.96 sqrt(V)`, retaining a bounded residual during totality. Material color and exposure are applied once before fog and tone mapping. Water's additional artistic sky reflection follows the same local response, as defined above.

The observer's own visibility fraction affects only the illustrative sky gradient and haze color. It is not applied a second time to the surface or global Sun light. Decorative stars preserve their ordinary night/altitude visibility; the eclipse adds visibility only when the CPU geometric classification is `Total`, using `1 − daylight × atmosphere × (1 − eclipse_stars)`, with `eclipse_stars = 0.96` during totality and zero otherwise. Partial and annular daylight do not expose the star field. This categorical rule changes at contact and is not a stellar-magnitude or eye-adaptation model.

The residual and sky response keep totality readable; they are not measured eclipse brightness, atmosphere or eye adaptation. Floating-point precision, finite render LOD, first-order spatial ephemeris extrapolation and Earth-orientation uncertainty also limit the displayed shadow. Neither screenshots nor the geometric HUD certify exact contact times or real-world photometry.

Independent scalar f32 execution of the shader expressions checks the actual `GpuSolarEclipse` packing against f64 geometry. The contact sweep's maximum absolute visible-fraction error was `1.952×10⁻⁷`. Spatial fixtures for two eclipse epochs, ground and 8,000 km anchors, and 15 km floating-origin rebases differed by at most `2.268×10⁻⁷`; an extreme 1×10⁹ m anchor reached `1.946×10⁻⁵`. These are measured fixture errors, not universal accuracy guarantees. The emulation excludes GPU fused arithmetic, vendor transcendental implementations and rasterization; native captures separately verify shader compilation and visible moving shadows.

## Review and reproducibility

The rendering mathematics is implemented by a mathematics subagent and independently checked in `crates/earth-math/tests/rendering_review.rs`, `boundary_review.rs` and the atmospheric review fixtures. Checks include analytic surface invariance, local-frame alignment, LOD selection, grass anchoring/range, shared regional/reference edge identity and winding, material-area conservation, analytic normals, profile integration and f32 display coefficients. Scalar shader transcriptions compare actual packed values with the independent f64 solver; renderer child tests inspect the real boundary buffer and region-uniform conversion. These tests exclude vendor shader arithmetic and actual raster sampling. Native graphics checks are additionally required for shader compilation, display scaling, shadows and optical views; numeric tests cannot validate their appearance. See [verification evidence](verification.md) for finite fixture results and their scope.
