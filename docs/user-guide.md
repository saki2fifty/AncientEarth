# Using Ancient Earth

## Navigation

The app opens in a generic globe overview. City catalogs, city search, map pins and named location shortcuts have been removed. GPS entry and free flight remain available. Select **Surface** to stand on the smooth surface at your current latitude and longitude.

Land and water share the **WGS84 ellipsoid at height zero**. There are no hills, seabed relief or topographic elevation maps in the current runtime. Earth's curvature remains. **Land & Water → Show mapped land / water** controls the regional classification layer; it does not change surface height. Outside the reviewed water-mask coverage, the surface is unclassified. Imagery remains removed. The base app works offline; optional live weather downloads are described below.

Select **Coast view**, or launch with `--coast`, to inspect a boundary derived from the loaded masks:

```powershell
$env:WGPU_BACKEND = "dx12"
.\target\debug\ancient-earth.exe --coast --datetime "2026-10-21 18:00:00" --paused
```

The map provides surface categories only. Its finite resolution still limits boundary detail, and it does not provide live tides or worldwide land/water coverage. See [the current surface model](science/water.md).

Open **Licenses & attribution** at the top of the navigation panel to read data and software/font notices. `ancient-earth.exe --licenses` prints them without opening a graphics window. Keep the license files included in packaged distributions when sharing the app.

Hold the right mouse button outside a panel to look. W/S follows your gaze; A/D moves sideways; E or Space rises and Q descends along local geodetic up. Shift boosts movement. Wheel scrolling changes flight speed exponentially. Escape, releasing the right button, or losing window focus releases mouse capture.

Hold **C** to crouch; release it to stand. **Observer & Display** also provides a persistent **Crouch** checkbox and separate standing/crouching eye-height settings. Keyboard flight and crouching are suspended while typing in GUI fields.

The observer's feet are constrained by the smooth WGS84 surface. There is no gravity or momentum. Flight is a navigation tool, not a physically valid aircraft or spacecraft simulation.

## Coordinates

Enter decimal-degree geodetic latitude and longitude: north and east are positive, south and west negative. Accepted angular ranges are latitude [-90, 90] and longitude [-180, 180]. Coordinate fields retain double precision.

**Feet height** is altitude above WGS84 in the selected unit. On this height-zero surface it also equals the feet's clearance; the geoid reference remains separate. **Go to exact coordinates** preserves the entered height and rejects a destination below the active surface. **Go to ground at coordinates** places the feet on the smooth surface at that latitude and longitude. Eye altitude adds the configurable eye height. Invalid coordinates report an error without moving the observer.

**Use current** copies the observer's current coordinates into the fields. Changing units changes displayed lengths, speed and height inputs while preserving the physical location.

## Eyes and distance

The observer is six feet tall. Standing eyes default to 1.72 m above the feet; crouching eyes default to 1.00 m. Both are adjustable conventions because total stature does not uniquely determine eye position. Crouching keeps the feet fixed and changes the actual camera origin, perspective and measurements.

The small center dot defines a single forward ray. The HUD identifies the nearest measured target: the smooth Earth surface, a visible grass blade or a modeled Sun/Moon sphere. Where mask coverage exists, the surface hit is labeled as land or mapped water. An unobstructed ray with no target reports **No surface intersection**. Measurements describe the mathematical model, not surveyed real-world terrain.

The vertical readout uses the point at your current latitude and longitude, even while flying. **Eyes above surface** includes flying clearance; the configured eye offset is only the height above your feet. **Eye WGS84 height** is ellipsoidal altitude. The separate **Datum reference / EGM2008** readout compares the observer and surface with the geoid. It does not move the rendered ocean or land and is not a live tide measurement.

The **skyline** readout follows the current eye position and heading. Turning, moving or crouching updates it. Its atmospheric mode and numerical limits are described in [scientific visibility](science/visibility.md) and [refraction](science/refraction.md).

## Weather and refraction

Open **Weather & Refraction** in the navigation panel and check **Enable weather refraction**. Unchecking it restores the geometric view and stops new weather requests. The feature starts disabled. Add `--weather` at launch to enable live weather immediately.

The panel shows the NOAA forecast model, source grid coordinates, run time, valid time and current download/cache/error status. **Refresh weather** requests an update. Moving to another source grid location invalidates the old profile before rendering. Downloads run in the background, with a short location debounce and a retry cooldown. A failed refresh can retain only an eligible previous profile; otherwise the geometric view remains active. A public forecast is not a weather-station measurement at your exact position.

Live weather follows **real computer UTC**, independently of the Sun/Moon calendar. Changing the calendar does not retrieve historical or future weather. The base scene still works without a connection. On Windows, the latest validated response is cached in `%LOCALAPPDATA%\AncientEarth\weather`; `ANCIENT_EARTH_WEATHER_CACHE` can select another directory. Cache bytes must match their recorded request and SHA-256, and pass scientific validation again before reuse.

For offline verification, `--weather-reference` selects the bundled, explicitly labeled fixed NOAA snapshot. It is never substituted silently after a live download fails. **Use live weather** exits this verification mode. Do not combine the two weather launch switches.

With the optical display active, the view and L/V guides use the optical model documented in [refraction](science/refraction.md). The distance HUD distinguishes curved geometric path, straight endpoint distance and phase optical path. Grass and its shadows are hidden in this mode because decorative blades do not yet support curved-ray visibility. Surface/sky lighting is simplified; disabling refraction restores the existing PBR lighting, grass and shadows. Thin local mirage layers, scattering and cloud obscuration are unresolved, so the app cannot guarantee the appearance or visibility of an actual mirage.

The optical display currently supports eyes up to **20 km above WGS84**. Above this range, the view, range readouts and L/V guides consistently use the geometric mode while weather data remain visible. A status identifies this fallback, including in F2 clean view. Within the supported range, an individual ray that exceeds the numerical budget or cannot resolve a thin layer is marked purple instead of inventing a hit. This is a display limit, not an atmospheric boundary.

Refraction is currently demanding: the documented stationary 1080p DirectX 12 debug capture on an RX 7900 XTX measured **12.7 FPS**, with panels and L/V hidden. Disabling the toggle restores the existing geometric renderer. See the [complete performance configuration](development/validation.md#optional-live-weather-and-clean-view-guides); this result is not a frame-rate guarantee for other views or hardware.

For a repeatable magnified horizon comparison, use `--weather-horizon-check --clean-view --line --paused --datetime "2026-10-21 18:00:00"`. Capture once without a weather option and once with `--weather-reference`. This diagnostic uses the independently reviewed source-column pose and lens, not a city catalog or a claim about current conditions.

## Optics

**Normal / no fisheye** is a rectilinear pinhole projection. It has no radial fisheye distortion; very wide rectilinear views still stretch near the edges. Lowering field of view reduces that effect.

**Equidistant fisheye** uses a circular aperture and angular radial projection. Pixels outside the aperture have no view ray.

**Telescope** uses rectilinear optics from the same eye position. The wheel changes field of view instead of movement speed. Normal and telescope settings are stored separately. Turning off the telescope restores the previous normal/fisheye lens.

The GPU camera uses a 0.02 m near clip and a finite 4×10¹² m culling distance, covering the supported Earth–Moon–Sun scene. Telescope zoom cannot create boundary detail absent from the source data. Field of view, f32 display precision and mesh tessellation all have finite limits. Extreme zoom can expose polygon edges or display precision even while f64 measurements remain stable. See [rendering mathematics](science/rendering.md).

## Display

The window starts at **1920 × 1080**. Press **F5** to use borderless fullscreen at the current monitor's resolution; press it again to restore the previous window size and position. The GPU draws at native resolution with antialiasing. **Visual detail**, in **Observer & Display**, controls surface rendering and grass coverage. Measurements use the authoritative smooth surface.

The app finishes preparing required GPU shaders before drawing a new view. Startup and the first switch to another optical mode can therefore pause briefly while shaders compile.

Windows can constrain a decorated window to its available work area. Automated `--screenshot` runs can add `--capture-fullscreen` to capture the current monitor's full resolution. Screenshot and frame-capture checks suppress gameplay mouse/keyboard input. Leave GUI controls untouched during these checks; ordinary launches remain fully interactive.

Tab hides the navigation panel while leaving measurements visible. **F2** hides panels and the center dot; press F2 again to restore them. **L and V remain independently available in clean view**, including their labels and Left/Right or drag adjustment. F2 preserves the guides' current enabled state. Flight and time playback continue. Start this way with `--clean-view`.

**Show grass**, in **Observer & Display**, lets you inspect the underlying smooth surface. The launch option `--no-grass` starts with grass hidden. `--quality performance`, `--quality balanced`, `--quality fine` and `--quality high` select the corresponding detail preset. The app currently keeps settings in memory for the current session only.

Press **L** to toggle the one-physical-pixel horizontal guide at the highest detected visible skyline. Its labels report both the straight-line span and WGS84 reference-surface distance between the sampled visible skyline endpoints, in the selected units. The guide is unavailable when no usable skyline is found. The same toggle is in **Observer & Display**.

With L enabled, press **V** to show the vertical guide. Hold **Left / Right arrow** or drag its marker to move it. Its label updates as it moves and when you change units. The label identifies the modeled gap at the selected skyline point's depth; it is not a surveyed terrain height or a direct measurement of Earth curvature. See the [guide definitions and limits](science/horizon-guides.md).

For a repeatable view of the horizon and both rulers, launch:

```powershell
.\target\debug\ancient-earth.exe --guide-demo horizon --vertical-guide --guide-column 0.8 --datetime "2026-10-20 12:00:00" --paused
```

Use `--guide-demo rolled` for a tilted view, or `--guide-demo sky` to check the unavailable-horizon message. `--imperial` selects imperial units at launch. These presets hide the navigation panel; press Tab to reopen it. In a packaged copy, run `ancient-earth.exe` directly from its extracted folder.

## Sun, Moon and time

The simulation starts at the current computer time. **Sun, Moon & Time** shows the simulation date and provides **Play / Pause**, **Now**, speed presets and an editable playback rate. `1x` advances in real time; a negative rate reverses playback.

Press **F9** to stop following Earth's rotation. Your view stays in the Earth-centered nonrotating frame as the planet turns beneath you. Press F9 again to follow Earth at your current location. Use **Play**, faster playback or time steps to see the effect; paused time holds both views still. Flight, looking and teleports remain available. **Observer & Display → Follow Earth rotation** controls the same setting, and `--earth-unlocked` starts with it unchecked. See [reference frames](science/reference-frames.md) for the model and collision limits.

**Aim at Sun** and **Aim at Moon** turn the view toward that body's current apparent center without moving you. The ground may obscure it. Use the telescope to magnify it. The corresponding launch switches are `--look-sun` and `--look-moon`.

Enter `YYYY-MM-DD HH:MM:SS` and select **Apply date / pause** to inspect a date. **Seconds per step** defines an interval; **Step count** may be positive or negative. **Step / pause** applies that many intervals and pauses playback. Years outside 1850–2149 are rejected rather than extrapolating the planetary ephemeris.

For repeatable launches, use `--datetime "2026-10-20 18:00:00" --paused`. The calendar uses UTC from 1960 onward and the identified historical model before then. The panel reports whether Earth rotation uses measured, predicted or modeled data. Future leap seconds and Earth rotation are not exactly knowable. See [scientific astronomy](science/astronomy.md) for the data, equations and independently checked limits.

Eclipse geometry and shadows remain active for your coordinates and selected date. The former named location demonstrations have been removed. See [the eclipse model and verification limits](science/eclipses.md).

## Recording frame pacing

For a repeatable stationary check, run a separate capture without `--screenshot`:

```powershell
$env:WGPU_BACKEND = "dx12"
& ".\target\debug\ancient-earth.exe" --coast --datetime "2026-10-20 18:00:00" --paused --frame-capture artifacts/performance/coast.json
python scripts/analyze_frame_capture.py artifacts/performance/coast.json
```

The app warms up, records raw application frame intervals and exits. Keep the window focused and leave the camera and display settings unchanged. The report includes the adapter, display settings and any detected view changes. It measures application frame pacing on that configuration, not GPU execution time.

For the automated movement and grass-regeneration check, add `--frame-capture-motion` to that launch command and choose a different output file. This mode moves the observer and changes display settings automatically, then exits. Its report identifies the run as a movement check, separate from a stationary performance baseline. See [the verification procedure](science/verification.md#reproducible-frame-pacing) for the checks and their limits.

## Windows startup diagnostics

Packaged Windows builds need the x64 Microsoft Visual C++ v14 runtime. If Windows reports missing `VCRUNTIME140.dll` or `VCRUNTIME140_1.dll`, install the x64 package from [Microsoft's supported redistributable downloads](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist). The runtime must be at least as recent as the MSVC build tools used for the app. It is a system prerequisite and is not bundled in Ancient Earth's ZIP.

Messages such as `loader_get_json: Failed to open JSON file C:\VulkanSDK\...\VkLayer_*.json` mean the Vulkan loader could not open a layer manifest. They do not by themselves mean the app failed: check subsequent messages for graphics adapter selection and window creation, and check whether the globe appears.

To bypass Vulkan and use DirectX 12, run these commands in PowerShell from the repository directory:

```powershell
$env:WGPU_BACKEND = "dx12"
& ".\target\debug\ancient-earth.exe"
```

For a release build, use `.\target\release\ancient-earth.exe` instead. Bevy reads `WGPU_BACKEND` when initializing graphics; the variable changes the backend for apps launched from this PowerShell session. Close that shell to discard the setting, or run `Remove-Item Env:WGPU_BACKEND` before the next launch to restore automatic selection. This workaround needs no rebuild or system configuration changes.

DirectX 12 was tested on 2026-10-20 with the existing Windows executable and an AMD Radeon RX 7900 XTX. That earlier revision rendered the globe and GUI, saved a screenshot, and exited successfully without the Vulkan manifest errors.

Gamepad messages about an unknown mapping or an unavailable controller are separate from graphics initialization. This app uses keyboard and mouse controls. A PowerShell upgrade notification is also separate from the app.

References: [Bevy graphics backend selection](https://docs.rs/bevy/latest/bevy/render/settings/struct.Backends.html) and [Khronos Vulkan layer manifests](https://github.com/KhronosGroup/Vulkan-Loader/blob/main/docs/LoaderLayerInterface.md).
