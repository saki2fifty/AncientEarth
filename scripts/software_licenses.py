"""Generate/check complete Windows dependency notices from locked Cargo sources.

Includes the resolved build graph (a conservative superset of linked code),
nested license/NOTICE files, C++ stb notices, embedded fonts and Rust stdlib.
New license expressions and missing texts fail closed for manual review.
"""
import argparse
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-pc-windows-msvc"
ALLOWED = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "BSL-1.0",
    "CC0-1.0", "0BSD", "Unlicense", "Unicode-3.0",
    # Reviewed webpki-roots trust-store data: use/modify/share permitted;
    # section 2.1 requires this complete license to accompany shared data.
    "CDLA-Permissive-2.0",
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def select_license(expression):
    normalized = expression.replace("/", " OR ")
    tokens = re.findall(r"\(|\)|[A-Za-z0-9.+-]+", normalized)
    if re.sub(r"\s+", "", normalized) != "".join(tokens):
        raise ValueError(f"Invalid license expression: {expression}")
    position = 0

    def term():
        nonlocal position
        if position >= len(tokens):
            raise ValueError(f"Invalid license expression: {expression}")
        if tokens[position] == "(":
            position += 1
            result = alternatives()
            if position >= len(tokens) or tokens[position] != ")":
                raise ValueError(expression)
            position += 1
            return result
        license_id = tokens[position]
        position += 1
        if position < len(tokens) and tokens[position] == "WITH":
            license_id += " WITH " + tokens[position + 1]
            position += 2
        return [license_id] if license_id in ALLOWED else None

    def conjunction():
        nonlocal position
        result = term()
        while position < len(tokens) and tokens[position] == "AND":
            position += 1
            other = term()
            result = result + other if result is not None and other is not None else None
        return result

    def alternatives():
        nonlocal position
        result = conjunction()
        while position < len(tokens) and tokens[position] == "OR":
            position += 1
            other = conjunction()
            if result is None:
                result = other
        return result

    result = alternatives()
    if position != len(tokens) or result is None:
        raise ValueError(f"License needs review: {expression}")
    return " AND ".join(result)


class ReadableHTML(HTMLParser):
    def __init__(self):
        super().__init__()
        self.parts = []
        self.hidden = 0

    def handle_starttag(self, tag, attrs):
        if tag in ("style", "script"):
            self.hidden += 1
        if tag in ("p", "pre", "h1", "h2", "h3", "h4", "li", "br", "div"):
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in ("style", "script"):
            self.hidden -= 1
        if tag in ("p", "pre", "h1", "h2", "h3", "h4", "li", "div"):
            self.parts.append("\n")

    def handle_data(self, data):
        if not self.hidden:
            self.parts.append(data)


def generate():
    command = ["cargo", "metadata", "--format-version", "1", "--locked", "--offline", "--filter-platform", TARGET]
    metadata = json.loads(subprocess.check_output(command, cwd=ROOT, encoding="utf-8"))
    active = {node["id"] for node in metadata["resolve"]["nodes"]}
    overrides = json.loads((ROOT / "licenses/vendor/sources.json").read_text(encoding="utf-8"))
    supplemental = json.loads((ROOT / "licenses/supplemental/sources.json").read_text(encoding="utf-8"))
    texts, packages = {}, []

    def record(label, content):
        text = content.decode("utf-8-sig").replace("\r\n", "\n").strip() + "\n"
        key = digest(text.encode("utf-8"))
        texts.setdefault(key, {"text": text, "sources": []})["sources"].append(label)
        return key

    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] not in active or not package["source"]:
            continue
        name = package["name"] + "@" + package["version"]
        selected = select_license(package["license"] or "")
        directory = Path(package["manifest_path"]).parent
        files = sorted(f for f in directory.rglob("*") if f.is_file()
                       and f.name.lower().startswith(("license", "licence", "copying", "copyright", "notice", "unlicense"))
                       and f.suffix.lower() not in (".rs", ".c", ".h", ".png", ".jpg"))
        references = []
        for file in files:
            references.append(record(name + "/" + file.relative_to(directory).as_posix(), file.read_bytes()))
        if not references:
            for source in overrides.get(name, []):
                content = (ROOT / source["path"]).read_bytes()
                if digest(content) != source["sha256"]:
                    raise ValueError(f"Changed upstream license: {name}")
                references.append(record(name + " — " + source["source_url"], content))
        if name == "constgebra@0.1.4":
            # Its published Cargo.toml explicitly grants MIT OR Apache-2.0 but
            # omits license files. Elect Apache-2.0; supply its standard text.
            if package["license"] != "MIT OR Apache-2.0":
                raise ValueError("constgebra license declaration changed")
            selected = "Apache-2.0"
            references.append(record(
                name + " — Apache-2.0 elected from the published Cargo.toml license declaration; no upstream NOTICE supplied",
                (ROOT / "licenses/supplemental/Apache-2.0.txt").read_bytes()))
        if not references:
            raise ValueError(f"Missing license text: {name}; review exact-version upstream sources")
        if package["name"] == "dear-imgui-sys":
            for header in sorted((directory / "third-party/cimgui/imgui").glob("imstb*.h")):
                content = header.read_text(encoding="utf-8")
                start = content.rfind("ALTERNATIVE A - MIT License")
                end = content.find("ALTERNATIVE B", start)
                if start < 0 or end < 0:
                    raise ValueError("stb license block changed; review required")
                references.append(record(name + "/" + header.name + " (MIT option)", content[start:end].encode()))
        packages.append({
            "name": package["name"], "version": package["version"],
            "declared_license": package["license"], "selected_license": selected,
            "repository": package["repository"], "license_texts": references,
        })
    for name, source in sorted(supplemental.items()):
        content = (ROOT / "licenses/supplemental" / name).read_bytes()
        if digest(content) != source["sha256"]:
            raise ValueError(f"Changed supplemental license: {name}")
        record(name + " — " + source["source_url"], content)
    runtime = ROOT / "licenses/runtime/COPYRIGHT-library.html"
    runtime_source = json.loads((ROOT / "licenses/runtime/source.json").read_text(encoding="utf-8"))
    toolchain = subprocess.check_output(["rustc", "--version"], encoding="utf-8").strip()
    if toolchain != runtime_source["toolchain"] or digest(runtime.read_bytes()) != runtime_source["sha256"]:
        raise ValueError("Rust toolchain or runtime notices changed; refresh and review the runtime license inventory")
    reader = ReadableHTML()
    reader.feed(runtime.read_text(encoding="utf-8"))
    record("Rust standard library and bundled runtime components / COPYRIGHT-library.html",
           "".join(reader.parts).encode("utf-8"))
    report = {
        "target": TARGET, "toolchain": toolchain,
        "cargo_lock_sha256": digest((ROOT / "Cargo.lock").read_bytes()),
        "scope": "Resolved Windows default-feature dependency graph, including build dependencies; not a license audit of other binary targets.",
        "packages": packages,
    }
    lines = [
        "Ancient Earth — third-party software and embedded font notices\n",
        f"Target: {TARGET}\n{toolchain}\n",
        "Generated from locked sources. Includes build-time dependencies as a conservative superset.\n"
        "Each package retains its own license. Selected alternatives below apply to this distribution.\n"
        "Additional license texts are preserved even when an alternative is not selected.\n",
    ]
    for package in packages:
        lines.append(f"{package['name']} {package['version']} — {package['selected_license']}\n"
                     f"  Upstream: {package['repository']}\n")
    for key, value in sorted(texts.items()):
        lines.append("\n" + "=" * 78 + "\n" + "\n".join(sorted(value["sources"]))
                     + f"\nText SHA-256: {key}\n\n" + value["text"])
    return "".join(lines), json.dumps(report, indent=2, ensure_ascii=False) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    text, report = generate()
    for path, content in [
        (ROOT / "THIRD_PARTY_LICENSES.txt", text),
        (ROOT / "licenses/software-manifest.json", report),
    ]:
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != content:
                raise SystemExit(f"License report is stale: {path.name}; regenerate and review")
        else:
            path.write_text(content, encoding="utf-8", newline="\n")
    print("Windows software/font notices verified" if args.check else "Generated Windows software/font notices")


if __name__ == "__main__":
    main()
