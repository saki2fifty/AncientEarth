param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release"
)
$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
Push-Location -LiteralPath $projectRoot
try {
    python scripts/check_asset_licenses.py
    if ($LASTEXITCODE -ne 0) { throw "Asset license audit failed" }
    python scripts/software_licenses.py --check
    if ($LASTEXITCODE -ne 0) { throw "Software license audit failed" }
    $buildArgs = @("build", "--locked", "--offline")
    if ($Profile -eq "release") { $buildArgs += "--release" }
    cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "Build failed" }
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss-fff"
    $name = "ancient-earth-windows-x64-$Profile-$stamp"
    $destination = Join-Path $projectRoot "artifacts/packages/$name"
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item -LiteralPath "target/$Profile/ancient-earth.exe" -Destination $destination
    foreach ($file in @("LICENSE", "NOTICE.md", "THIRD_PARTY_LICENSES.txt", "README.md", "Cargo.lock")) {
        Copy-Item -LiteralPath $file -Destination $destination
    }
    Copy-Item -LiteralPath "licenses" -Destination $destination -Recurse
    Copy-Item -LiteralPath "docs" -Destination $destination -Recurse
    $provenanceDirectory = Join-Path $destination "asset-provenance"
    New-Item -ItemType Directory -Path $provenanceDirectory | Out-Null
    foreach ($file in Get-ChildItem -LiteralPath "assets" -Recurse -File -Filter "*.provenance.json") {
        Copy-Item -LiteralPath $file.FullName -Destination $provenanceDirectory
    }
    @"
Ancient Earth

Run ancient-earth.exe for the globe overview or ancient-earth.exe --coast for a coast view.
Land and water share the smooth WGS84 ellipsoid; no topographic elevations are loaded.
The regional water masks provide boundaries and categories only.
Outside mask coverage the surface is unclassified. No tides are simulated.
City catalogs, map pins and named location shortcuts have been removed. GPS entry remains.
Hold C to crouch. Press L to toggle the one-pixel skyline guide.
With L enabled, V shows a vertical measurement; move it with Left/Right or drag.
F9 toggles following Earth rotation. Play or step time to see Earth turn underneath.
Press F5 to switch between the 1080p window and native-monitor fullscreen.
Press F2 to hide or restore panels and the dot. L/V remain usable in clean view.
Sun, Moon & Time provides calendar entry, play/pause, speed and signed time steps.
Eclipse geometry and shadows use your coordinates and selected date.
Observer & Display provides eye-height settings and a persistent crouch toggle.
Its Show grass checkbox lets you inspect the smooth surface underneath.
EGM2008 is a separate datum readout; it does not displace the land or water.
Weather & Refraction provides an enable/disable toggle (off by default).
Use --weather for live NOAA forecasts at your in-app location and real UTC.
Use --weather-reference for the explicitly labeled fixed verification snapshot.
Refraction mode uses simplified lighting and hides decorative grass and its shadows.
Above 20 km eye height, view and measurements explicitly return to geometric mode.
Weather profiles have finite resolution and do not guarantee actual mirage conditions.
Use --licenses to print embedded notices without starting graphics.
The GUI also has a Licenses & attribution window.

In PowerShell, if Vulkan startup reports stale SDK layers:
  `$env:WGPU_BACKEND = "dx12"
  .\ancient-earth.exe --coast

Base scene data are embedded. Optional live weather requires NOAA network access.
System prerequisite: x64 Microsoft Visual C++ v14 runtime (not bundled).
If VCRUNTIME140.dll or VCRUNTIME140_1.dll is missing, install the x64 runtime:
https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist
Keep LICENSE, NOTICE.md, THIRD_PARTY_LICENSES.txt and licenses/ with redistributed copies.
See docs/user-guide.md and docs/development/licensing.md.
"@ | Set-Content -LiteralPath (Join-Path $destination "START-HERE.txt") -Encoding utf8
    $archive = "$destination.zip"
    Compress-Archive -LiteralPath $destination -DestinationPath $archive
    $checksum = Get-FileHash -LiteralPath $archive -Algorithm SHA256
    "$($checksum.Hash.ToLower())  $name.zip" | Set-Content -LiteralPath "$archive.sha256" -Encoding ascii
    Write-Output "Packaged: $archive"
}
finally {
    Pop-Location
}
