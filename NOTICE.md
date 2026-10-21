# Third-party data

The project's MIT license covers its original software. The third-party data below retain their own licenses and attribution requirements. Include this notice with distributions containing the bundled assets, including executable-only releases.

The current application uses regional water-mask boundaries on a smooth WGS84 surface. It no longer loads topographic elevations, photographic imagery or a city catalog. Historical assets and screenshots retained in this repository and documentation keep their original source notices below. The EGM2008 grid supplies a separate datum readout only.

## Copernicus terrain and water

The archived Tokyo, Mount Fuji, Houston and Galveston elevation meshes are derived from the Copernicus GLO-30 Public digital surface model, AWS 2021 release, accessed on 2026-10-20 from the [AWS Open Data registry](https://registry.opendata.aws/copernicus-dem/). They are no longer loaded by the application.

License: [Copernicus WorldDEM-30 general-public license, PDF pages 21–23](https://s3.waw3-1.cloudferro.com/swift/v1/portal_uploads_prod/CSCDA_ESA_Mission-specific_Annex_31_Oct_22_latest.pdf#page=21). Articles 4–6 permit reproduction, distribution, public communication and modification without a license fee, subject to the stated obligations. Recipients who redistribute or publicly communicate this data, including modified data, must retain the required attribution and liability notice, avoid implying official endorsement, and pass these obligations to subsequent redistributors.

Required modified-data notice:

> produced using Copernicus WorldDEM-30 © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA; all rights reserved

Required liability notice:

> The organisations in charge of the Copernicus programme by law or by delegation do not incur any liability for any use of the Copernicus WorldDEM-30.

Ancient Earth is not officially endorsed by the data provider, licensor or organisations responsible for the Copernicus programme.

See the [Copernicus product, license and citation guidelines](https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM). Archived elevation modifications: cropped four local regions, retained every third source post, converted EGM2008 heights to WGS84 ellipsoidal heights, and triangulated the retained data. Source and derivative hashes are in `assets/terrain/*.provenance.json`.

The elevation conversion uses the public-domain NGA EGM2008 geoid grid distributed by [PROJ-data](https://github.com/OSGeo/PROJ-data/blob/master/us_nga/us_nga_README.txt).

The regional Water Body Masks come from the `AUXFILES` directories of the same four AWS products, retrieved on 2026-10-20. Their accompanying `INFO/eula_F.pdf` licenses were reviewed individually and are byte-identical. The complete original license is retained in [licenses/Copernicus-WorldDEM-30-eula_F.pdf](licenses/Copernicus-WorldDEM-30-eula_F.pdf); exact download URLs and checksums are recorded in [licenses/water-sources.json](licenses/water-sources.json). These water derivatives carry the same required modified-data notice, liability notice and onward redistribution obligations above. The masks are geographic data, not photographic imagery. Processing and derivative hashes are recorded in `assets/terrain/*.water.provenance.json` and [water documentation](docs/science/water.md).

Archived regional surface derivatives retain original sampled DSM post heights after EGM2008-to-WGS84 conversion. Ocean coastal cells additionally retain native DEM/WBM posts. Ordinary binary source cells use their outer-edge category midpoints joined by declared cell-contained cubic curves; ambiguous checkerboards retain an explicit diagonal connectivity rule. New crossings and curve samples are modeled at H_EGM2008=0. Shared edge subdivisions, curve triangulation and Cartesian stitching centers change interpolation between source posts. Inland category mixtures remain conservative. These historical derivatives are neither surveyed coastlines nor live tide or bathymetric data.

Current boundary assets are derived solely from the licensed Copernicus Water Body Masks: four registered crops retain source categories, reconstruct ocean interfaces with contained cubic curves and conservative inland-water classification, and store height-free geographic parameters. All runtime land/water geometry is evaluated on the smooth WGS84 ellipsoid at h=0. No DSM elevations, geoid displacement or photographic data enter this boundary preparation. The reconstruction changes categorical interpolation, not source measurements, and does not claim a surveyed shoreline. Source and derivative hashes are in `assets/boundaries/region-*.provenance.json`; the `.aeb` files carry the required credits, original license and redistribution obligations above.

## NOAA atmospheric weather data

Credit: **NOAA / NCEP Global Forecast System (GFS)**. Live profiles use NOAA-produced GFS data from the official NOMADS service. The [NOAA/NWS data terms](https://www.weather.gov/disclaimer) place NOAA-produced information in the public domain unless specifically noted otherwise, permitting lawful use, modification and redistribution, including commercial use. The [NOAA-managed GFS distribution entry](https://registry.opendata.aws/noaa-gfs-bdp-pds/) also permits use and requests attribution. Third-party products hosted by NOAA are not covered by this approval and are not used by this integration.

The exact reviewed terms are retained in [licenses/NOAA-NWS-data-terms.html](licenses/NOAA-NWS-data-terms.html). Retain the NOAA/NCEP credit, source URL, model run, valid time and modification description with redistributed weather data and derived captures. Do not claim ownership of the original NOAA data, imply NOAA endorsement or affiliation, or present Ancient Earth's modified atmospheric scenarios as official NOAA products. Live data availability and forecast accuracy are not guaranteed.

Modifications: NOMADS supplies a geographic and variable subset. Ancient Earth decodes the original GRIB fields and constructs its own explicitly labeled optical scenario. The runtime cache preserves source URL, retrieval time, SHA-256 and these reuse conditions. It is local runtime data, not a bundled live forecast. Any fixed verification samples or screenshots distributed by the project are separately registered in `licenses/assets.json` with their provenance.

The GRIB decoder also uses MIT-licensed WMO Common Code Tables and GRIB2 code tables. Their exact bundled revision licenses are preserved in `licenses/supplemental/WMO-CCT-MIT.txt` and `WMO-GRIB2-MIT.txt` and in `THIRD_PARTY_LICENSES.txt`. The HTTPS trust-store data uses CDLA-Permissive-2.0; its complete terms accompany the software notices. No fonts were changed for weather support.

## Global sea-level reference

Credit: **NGA EGM2008 / GeographicLib / PROJ-data**. PROJ-data explicitly identifies `us_nga_egm08_25.tif` as public domain. This permits use, modification and redistribution, including commercial redistribution. The bundled grid is a losslessly repackaged copy of its worldwide posts, used offline for the EGM2008 mean-sea-level reference. The preprocessing script and asset provenance record the source, conversion and hashes. This reference does not provide live tides or water levels, and no agency endorsement is implied.

## GSI aerial imagery

Created by editing **GSI Tiles (Geospatial Information Authority of Japan)**.

Source: [GSI seamless aerial photography](https://maps.gsi.go.jp/development/ichiran.html#seamlessphoto), zoom 14, retrieved 2026-10-20. See the [GSI terms of use](https://www.gsi.go.jp/ENGLISH/page_e30286.html). The tile catalog lists this photography among the content usable with attribution. The two included regions do not intersect the separately identified GRUS imagery area.

License: [Public Data License 1.0](https://www.digital.go.jp/en/resources/open_data/public_data_license_v1.0), with the GSI-specific terms linked above. It permits copying, public transmission, modification and commercial use subject to its conditions, including source attribution, identification of edits and respect for third-party rights.

Modifications: stitched native north-up Web Mercator tiles into two JPEG mosaics, without reprojection or resizing. These are Ancient Earth's derivatives, not products created or endorsed by GSI. Individual source URLs, hashes, bounds and processing details are in `assets/textures/*-aerial.provenance.json`. Photography has mixed acquisition dates; download dates do not establish when the photographs were taken.

## Houston and Galveston aerial imagery

Credit: **USGS, USDA, The National Map: Orthoimagery. January 09, 2025.**

The Houston and Galveston mosaics use **2022 USDA National Agriculture Imagery Program (NAIP)** photography, downloaded on 2026-10-20 from the [official USGS NAIP Imagery service](https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer). The service explicitly identifies its NAIP imagery as public domain. See the [USGS NAIP product description](https://www.usgs.gov/centers/eros/science/usgs-eros-archive-aerial-photography-national-agriculture-imagery-program-naip). The service's credit date is not the photography date.

Modifications: requested natural-color, bilinearly resampled Web Mercator regional exports, preserving the original returned PNG bytes and transparency. Source footprint identifiers, acquisition metadata, export requests, actual returned bounds and SHA-256 hashes are recorded in `assets/textures/{houston,galveston}-aerial.provenance.json`. Missing photographic coverage uses the separately credited NASA global image. No USGS or USDA endorsement is implied.

## NASA Blue Marble

Credit: **NASA Earth Observatory**.

Source: [Blue Marble: Next Generation, July 2004 base map](https://science.nasa.gov/earth/earth-observatory/blue-marble-next-generation/base-map/), retrieved 2026-10-20. The global image was resized while retaining its full geographic bounds. Processing and hashes are in `assets/textures/blue-marble.provenance.json`. This is a historical monthly composite, not current satellite imagery.

Use follows the [NASA images and media guidelines](https://www.nasa.gov/nasa-brand-center/images-and-media/): factual Earth visualization with source acknowledgment and no implied NASA endorsement. This selected NASA Earth Observatory image has no third-party copyright credit, NASA logo or identifiable person. These statements do not grant rights to unrelated NASA website content, logos or third-party imagery.

## GeoNames

Historical city-pin screenshots and scientific test fixtures contain information derived from [GeoNames](https://www.geonames.org/), licensed under [Creative Commons Attribution 4.0](https://creativecommons.org/licenses/by/4.0/). The city dataset, importer, application catalog and pin functionality have been removed. This attribution remains for retained historical material.

Historical source: [cities15000.zip](https://download.geonames.org/export/dump/cities15000.zip), retrieved 2026-10-20.

Changes: selected 50 eligible populated-place records by population; converted to JSON; omitted unrelated fields; retained source latitude/longitude strings, identifiers and modification dates. Source data is provided without a guarantee of accuracy or completeness.

Historical eclipse screenshots and retained numerical test fixtures use the GeoNames representative place records for [Dallas, ID 4684888](https://www.geonames.org/4684888/dallas.html), [Albuquerque, ID 5454711](https://www.geonames.org/5454711/albuquerque.html), and [Houston, ID 4699066](https://www.geonames.org/4699066/houston.html), reviewed 2026-10-20 under the same CC BY 4.0 terms. Named location demonstrations have been removed from the application. These source points do not uniquely define a city's geographic center.

## Natural Earth

Made with Natural Earth. Source: [Natural Earth](https://www.naturalearthdata.com/). The Natural Earth I map is [public-domain data](https://www.naturalearthdata.com/about/terms-of-use/).

Source: [Natural Earth I, 1:50m, shaded relief and water](https://www.naturalearthdata.com/downloads/50m-raster-data/50m-natural-earth-1/). The bundled image is a resampled JPEG derivative; exact processing and hashes are recorded in `assets/textures/earth.provenance.json` and `docs/science/imagery.md`.

## Software

### JPL planetary ephemeris and Earth orientation

Credit: **NASA / JPL / Caltech, JPL DE440 ephemeris team, NAIF / PDS**.
`assets/astronomy/de440s.bsp` is redistributed unchanged from the
[NAIF generic kernel archive](https://naif.jpl.nasa.gov/pub/naif/generic_kernels/spk/planets/de440s.bsp).
Its original producer comments remain embedded. The [NAIF rules](https://naif.jpl.nasa.gov/naif/rules.html)
permit kernel redistribution and commercial use. Modified kernels require a
different filename, appropriate validation and metadata, and replacement of the
producer attribution with the modifier's identity as specified by those rules.
The full retrieved rules and credit guidance are included in `licenses/astronomy/`.
No SPICE Toolkit is bundled; Ancient Earth evaluates this data with its own Rust reader.
No agency endorsement or warranty is implied.

Credit: **IERS / USNO Earth orientation data via Astropy Developers**.
`assets/astronomy/finals2000A.all` is an unchanged file from the pinned
Astropy `astropy-iers-data` distribution. That project explicitly licenses its
distribution under BSD-3-Clause. Copyright (c) 2023, Astropy Developers.
Full terms and the publisher's license declaration are retained in
`licenses/astronomy/astropy-iers-data-LICENSE.rst` and
`licenses/astronomy/astropy-iers-data-README.rst`; source revision and hash are
recorded in the asset provenance. Preserve these notices; do not imply endorsement.

Astronomical orientation code and coefficient tables are translated from
**ERFA 2.0.1**, copyright (c) 2013–2021, NumFOCUS Foundation, under its BSD terms.
This is a modified Rust implementation derived from ERFA, which in turn derives
from SOFA; it is not the unmodified SOFA reference implementation. Full terms
are included in `licenses/ERFA-LICENSE.txt` and the embedded software notices.

### Application dependencies

Rust dependencies retain their respective licenses. [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt) contains the complete collected notices for the locked Windows build, including nested C++/stb code, embedded fonts and the Rust standard library. [licenses/software-manifest.json](licenses/software-manifest.json) records versions, declared licenses and selected permitted alternatives.

The WGS84 surface-distance solver uses **geographiclib-rs 0.2.7**, the MIT-licensed Rust port of GeographicLib. Its complete upstream license is preserved in `THIRD_PARTY_LICENSES.txt`; the locked source and license hash are recorded in `licenses/software-manifest.json`. No optional polygon-accumulation dependency is enabled.

Bevy and dear-imgui-rs are MIT OR Apache-2.0; Dear ImGui and cimgui are MIT. Embedded ProggyClean is MIT, copyright (c) 2004, 2005 Tristan Grimmer; ProggyForever is MIT, copyright (c) 2026 Disco Hello and (c) 2019, 2023 Tristan Grimmer. Bevy's embedded Fira Mono is under SIL Open Font License 1.1, copyright (c) 2012–2015 The Mozilla Foundation and Telefonica S.A. Full font license texts are included in the software notice and `licenses/supplemental/`. Font software retains its font license; the application does not relicense it under MIT.

Distribute `LICENSE`, `NOTICE.md`, `THIRD_PARTY_LICENSES.txt` and `licenses/` with the app. Full data/software attribution is also available in the app's **Licenses & attribution** window and through `ancient-earth.exe --licenses`. Asset terms continue to apply to screenshots and other derived outputs.
