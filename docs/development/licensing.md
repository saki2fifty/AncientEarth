# Asset licensing and distribution

Only use assets whose actual source terms permit use, modification and redistribution, including commercial redistribution. A free download, public API or recognizable provider name is not enough. Unknown terms, noncommercial-only terms, no-derivatives restrictions and incompatible redistribution restrictions exclude an asset from this project.

Current regional boundaries use Copernicus Water Body Masks under the WorldDEM-30 general-public license; topographic elevation files are historical only. The global EGM2008 geoid uses public-domain NGA data distributed through GeographicLib and PROJ-data. Historical GeoNames assets remain CC BY 4.0; city logic is removed. JPL DE440s is redistributed unchanged under the reviewed NAIF kernel terms, including commercial-use permission and specific requirements for any modified kernels. Earth-rotation data comes from a pinned BSD-3-Clause Astropy IERS-data distribution; public access to an arbitrary IERS server is not the basis for its license review.

Optional live weather uses NOAA/NCEP-produced GFS fields from NOMADS. The actual NWS terms were reviewed and preserved in `licenses/NOAA-NWS-data-terms.html`; their public-domain permission permits commercial redistribution and modification, with attribution and a clear distinction between the official source and this derived scenario. Each runtime cache response records its URL, retrieval time, SHA-256, terms and modification notice. Cached responses are not included in packages. The fixed verification GRIB and its provenance are separately registered in `licenses/assets.json`. They are never an automatic live-weather fallback.

The current renderer uses original procedural materials. Retained historical photography and screenshots keep their original licenses: GSI Public Data License 1.0, public-domain USDA NAIP served by USGS, NASA factual-imagery reuse terms, and public-domain Natural Earth. These photographs are not loaded by the current renderer. Exact sources, notices and changes appear in [NOTICE.md](../../NOTICE.md).

Regional water masks use the Copernicus product's own `AUXFILES` components. The review covers each exact product's included `INFO/eula_F.pdf`, not merely the availability of its download URL. All four original licenses are identical; [water-sources.json](../../licenses/water-sources.json) records their hashes and the reviewed rights and obligations. The [original license PDF](../../licenses/Copernicus-WorldDEM-30-eula_F.pdf) accompanies Windows distributions. `python scripts/fetch_water_sources.py` retrieves only these pinned source files and fails if provider bytes have changed. It does not automatically approve a changed source.

## Adding or changing assets

1. Read the source-specific license and confirm rights to redistribute, modify and use commercially. Check any third-party exceptions and geographic restrictions.
2. Record the exact source URL, retrieval date, acquisition information when available, transformations and checksums in the asset's provenance.
3. Add all required attribution, modification and liability notices to `NOTICE.md`. Do not imply provider endorsement or apply the project's MIT license to third-party data.
4. Add the reviewed asset and provenance hashes to [licenses/assets.json](../../licenses/assets.json). Screenshots are derived assets and retain the underlying data/font attribution.
5. Run `python scripts/check_asset_licenses.py`. It rejects new files, changed bytes, missing provenance, unreviewed providers and missing attribution.

This inventory is a record of a source review, not an automatic legal determination. A hash change requires review; updating a hash alone does not establish permission.

## Software and fonts

[THIRD_PARTY_LICENSES.txt](../../THIRD_PARTY_LICENSES.txt) is generated from the locked Windows dependency graph, including build dependencies as a conservative superset. It retains package and nested license/NOTICE files, stb notices, ProggyClean/ProggyForever/Fira Mono font licenses and Rust standard-library notices. Missing crate license files are supplemented from the exact Git revision recorded by Cargo, with URLs and hashes in `licenses/vendor/sources.json`.

The ERFA-derived Rust astronomy code retains the upstream NumFOCUS copyright and full BSD terms, both in source and in the embedded/generated notices. It is identified as a modified derivative of ERFA, not the unmodified SOFA standard. Astropy's complete BSD terms and NAIF's full retrieved rules are also embedded through the supplemental notice inventory.

GRIB decoding uses pure-Rust codec features. Its embedded WMO common and GRIB2 tables retain the original MIT terms from the exact source revisions in `licenses/supplemental`. Rustls's `webpki-roots` data uses the reviewed CDLA-Permissive-2.0 terms; the complete agreement accompanies the redistributed trust data through the software notice inventory. These are source-specific reviews, not an exemption from dependency auditing. No font assets changed for the weather feature.

`constgebra` 0.1.4 declares `MIT OR Apache-2.0` in its published Cargo manifest but omits license text. This distribution elects its declared Apache-2.0 alternative and supplies the official Apache license text; no copyright holder is invented. This is an explicit reviewed exception, not a rule permitting unlicensed packages.

Run:

```powershell
python scripts/software_licenses.py
python scripts/software_licenses.py --check
```

Review generated changes before accepting a dependency update. The generator rejects license expressions outside the reviewed permissive set, missing text, changed supplemental notices and changed Rust runtime versions. It records selected alternatives; an unselected LGPL alternative does not make the distribution LGPL.

The software inventory currently covers `x86_64-pc-windows-msvc` with default features. Other binary targets need their own dependency inventory before distribution. Python data-preparation dependencies stay in the development environment and are not bundled in the native application.

## Windows packages

```powershell
pwsh -File scripts/package_windows.ps1 -Profile debug
```

The packager runs both license checks, builds the executable, and makes a ZIP containing the executable, original license, data notices, complete software/font notices, license inventory and user documentation. It does not distribute development caches or optional Python packages.

The app embeds data and software notices. Open **Licenses & attribution**, or run `ancient-earth.exe --licenses` to print them without starting the renderer. Preserve the accompanying notices when redistributing a package or derived media.
