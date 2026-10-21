# Architecture

## Components

| Location | Responsibility |
| --- | --- |
| `crates/earth-math` | f64 WGS84 geodesy, analytic ellipsoid rays, geographic boundary classification, projections, distance, flight, units and astronomy; GeographicLib supplies inverse geodesics |
| `src/surface.rs` | Bundled height-free boundary loading, appearance selection and source labels |
| `src/datum.rs` | Offline EGM2008 datum comparison only; never changes the h=0 scene geometry |
| `src/navigation.rs` | Renderer-independent GPS navigation and command validation |
| `src/desktop/mod.rs` | Bevy setup and screenshot mode |
| `src/desktop/controls.rs` | Bevy input translation and GUI capture policy |
| `src/simulation.rs` | Bundled ephemeris/Earth-orientation loading, clock and astronomy snapshots |
| `crates/earth-math/src/astro/eclipse.rs` | Spatial solar-eclipse model and readout formatting |
| `crates/earth-math/src/astro/reference_frames.rs` | Observer frame tracking and rotation-related clearance checks |
| `crates/earth-math/src/guides.rs`, `geodesic.rs` | Ellipsoid horizon guides and reference-surface distances |
| `src/desktop/renderer.rs` | Native GPU meshes, floating origin, celestial visuals, grass, lights and optical camera views |
| `src/desktop/renderer/materials.rs` | Original ground/water PBR extensions, eclipse uniforms, bounded GPU grass geometry and lunar material |
| `src/desktop/renderer/refraction.rs`, `src/desktop/shaders/refraction.wesl` | Native-pixel curved viewing rays, analytic ellipsoid shading and source-boundary lookup |
| `crates/earth-math/src/atmosphere.rs`, `atmosphere/gpu.rs` | Validated phase-index profiles, f64 optical integration and f32 local-geometry packing |
| `src/weather.rs`, `src/weather/transport.rs` | Optional weather acquisition, profile eligibility, cache and visible provider status |
| `src/desktop/frame_capture.rs` | Opt-in native frame pacing capture with configuration metadata |
| `src/desktop/shaders/fisheye.wesl` | Original equidistant reprojection of six GPU camera views |
| `src/desktop/shaders/ground.wesl`, `grass.wesl`, `water.wesl`, `moon.wesl` | Filtered ground/water detail, shared grass color/shadow deformation and lunar phase lighting |
| `src/desktop/shaders/eclipse.wesl` | Shared spatial Sun/Moon occultation, embedded into ground, grass and water shaders |
| `crates/earth-math/src/rendering.rs`, `rendering/boundaries.rs`, `rendering/grass.rs` | Audited display tessellation, shared region edges, grass placement and sight intersection |
| `src/desktop/ui.rs` | Dear ImGui navigation, clock, presentation controls and sight HUD |
| `assets` | Offline boundary and astronomical data, provenance and license notices |
| `scripts` | Reproducible asset preparation, verification and packaging |
| `docs/science` | Scientific definitions, primary sources and review evidence |

## Precision and surface model

The physical Earth surface is the analytic WGS84 ellipsoid at `h=0` everywhere, including land, ocean, lakes and rivers. Runtime boundary assets retain geographic land/water partitions but contain no topographic heights. They do not displace the surface. The reference ellipsoid is an idealized model, not local mean sea level, a geoid or real topography. The separate EGM2008 readout remains a datum comparison at the observer's location; it does not move land, water, roots or the horizon.

Scientific geography, optical ray queries, collision and astronomy live in `earth-math`. Mathematical presentation formulas are also implemented by a mathematics subagent and independently reviewed. Dear ImGui draws controls over the GPU view at native framebuffer resolution.

Scientific measurements intersect the analytic f64 ellipsoid and classify the resulting geographic point using the retained boundaries. GPU display triangles are finite chords of that ellipsoid. An origin near the observer is subtracted from ECEF positions in f64; only the resulting local east/up/south coordinates are converted to f32. Movement rebases the meshes when needed. Scientific queries remain independent of display LOD and visual materials.

Regional material meshes retain their full source-derived partition. All vertices use `h=0`, with the same analytic ellipsoid normal on both sides of a shoreline. The global reference mesh uses adaptive geographic tessellation. A joint builder unions regional perimeter posts with adaptive reference endpoints, then uses identical shared edge subdivisions on both sides. Regional outer faces that need new edge posts are fan-triangulated in their geographic parameter plane and projected to `h=0`; their material classes are unchanged. This removes region-edge T-junctions without overlapping skirts or height offsets. See [rendering mathematics and limits](../science/rendering.md).

The regional boundaries retain the source classification and bounded coastal reconstruction documented in [water reconstruction](../science/water.md). They remain finite-resolution regional data. Unclassified areas use a neutral illustrative land appearance, without claiming a complete global land/water mask. Mixed and water regions suppress grass. No photographic imagery or official topographic elevation is loaded into the active scene. The legacy sampled-terrain math and source verification can remain in the repository for historical tests, but are not the application's physical surface.

## Appearance and animation

Ground detail is a shading-only PBR extension with periodically anchored ECEF coordinates, derivative filtering and finite near-detail range. Original grass roots use the exact ellipsoid and boundary exclusion policy. Its resting geometry and bounded wind coefficients are prepared in a background task; color and shadow shaders apply the same frame uniform and observer-dependent fade. Explicit culling bounds include every wind pose. Meshes, blade records and sight-query bounds swap together after checking scene, origin, quality and observer validity. The Moon likewise retains a static sphere and updates an illumination uniform during clock playback.

Water uses an opaque PBR extension with filtered periodic shading-normal ripples and an illustrative reflection of the original sky gradient. Only bounded phase uniforms change during animation. Water vertices, analytic range targets and the geometric horizon do not move. Colors, roughness, reflection and ripple frequencies are appearance choices, not measured local water optics or hydrodynamics. Finite shadow maps and MSAA apply to the fixed display geometry.

The astronomy snapshot supplies a receive-time solar-eclipse field. The renderer evaluates local solar visibility at every land, ellipsoid, grass and water fragment in a Sun-aligned coordinate frame. The observer's illustrative sky response is separate from this spatial attenuation and from nearby shadow maps. Shared shader source prevents surface occultation formulas from diverging.

An eligible active weather profile switches the renderer to a native-resolution curved-ray pass. It launches the selected lens directions, integrates the profile, intersects analytic WGS84 h=0 and classifies the actual hit in the retained regional triangles. Finite Sun/Moon rays continue from the atmosphere exit point and tangent. CPU sight and L/V measurements use the same profile and optical equations with separate f64 numerical controls. `WeatherState::view_profile(observer)` selects this mode consistently for the renderer and measurements: outside the current 0–20 km geodetic eye-height domain, all use geometric rays with an explicit fallback notice while retaining weather data. Grass is hidden in optical mode because the raster blades do not have a curved-ray intersector. Geometric mode retains the existing PBR cameras, grass and shadow maps. Fog and sky radiance remain explicitly artistic; weather-index ray bending is distinct from a full scattering/cloud/photometry model. See [weather and refraction](../science/refraction.md) and [display precision](../science/rendering.md).

The executable embeds required data and software/font attribution. `--licenses` prints it without starting graphics. Source-reviewed asset hashes and the locked Windows dependency license inventory are checked before packaging; see [licensing and distribution](licensing.md).

## Frame order

The Dear ImGui pass follows Bevy input collection:

1. GUI actions update GPS navigation, clock and lens settings.
2. Input handling applies permitted flight, look and zoom actions.
3. The clock updates the observer's selected reference frame and astronomy snapshot.
4. Weather eligibility is checked against the new observer location and real UTC; old-location profiles are removed before rendering.
5. GPU presentation updates camera transforms, required display meshes, grass and celestial lighting, or the active curved-ray pass.
6. The HUD measures the same observer pose and draws the sight readout and optional guides. F2 hides panels and dot independently of L/V.

UI capture blocks gameplay input while editing fields or using panels. Focus loss releases the mouse. Crouch updates the math library's eye posture before rendering and measurement. F9 selects whether the observer follows Earth, transporting the complete camera pose before rendering. L/V guide measurements are independent of GPU LOD and drawn in physical framebuffer pixels. See [reference frames](../science/reference-frames.md) and [horizon guides](../science/horizon-guides.md).

## Dependencies and verification

Bevy `=0.20.0` and `dear-imgui-bevy` / `dear-imgui-rs` `=0.21.0` are pinned as a compatible release train. The lockfile pins the resolved tree. Rust 1.97.1 is the backend's minimum compiler. The math library has no rendering dependency and can be tested independently. Bevy manages ECS, input, window, GPU materials, shadow maps and antialiasing; Dear ImGui supplies the interactive panels. The baseline surface/astronomy scene operates offline; live weather is an explicit network-backed option, with provider and cached-data state visible to the user.

`RenderPlugin::synchronous_pipeline_compilation` is enabled. Native tests found asynchronous preparation could leave newly selected views incomplete during startup; synchronous preparation trades a first-use pause for a complete view. Screenshot checks also wait for presentation readiness and UI layout.

Run the scientific tests before desktop checks. Capture ground, coast, fisheye and telescope views after rendering or input changes. Numerical tests verify analytic intersections, classification, source registration and shared-edge display topology; screenshots separately check shader compilation, visibility and visual continuity. Neither establishes real-world survey accuracy for the idealized model.
