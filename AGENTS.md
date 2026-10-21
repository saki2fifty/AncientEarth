# Project requirements

- Use only assets with verified rights to use, modify and redistribute in this project, including commercial redistribution. Public download access alone is not permission. Reject assets with unknown, noncommercial, no-derivatives or incompatible redistribution terms.
- Preserve provider credits, modification notices, license terms and source provenance. Add every new asset or screenshot to `licenses/assets.json` after reviewing the actual source's terms. Do not weaken the audit to admit an unreviewed asset.
- Run `python scripts/check_asset_licenses.py` before builds intended for distribution. Use `scripts/package_windows.ps1` for Windows distribution so required notices accompany the executable.
- Review dependency and embedded-font licenses when updating dependencies; regenerate `THIRD_PARTY_LICENSES.txt` with `python scripts/software_licenses.py`, inspect the result, and run its `--check` mode.
- All mathematical work must be performed by a mathematics subagent and independently reviewed, including geodesy, projections, coordinate handling, distances, terrain preprocessing and scientific documentation.
