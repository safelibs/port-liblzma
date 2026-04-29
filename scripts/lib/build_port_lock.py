"""Synthesize a port-04-test deb lock + override-deb-root for the safelibs
validator using the .deb files this repo just built into dist/.

Driven entirely by environment variables set by run-validation-tests.sh:

  SAFELIBS_LIBRARY        library identifier (matches validator manifest)
  SAFELIBS_COMMIT_SHA     full commit SHA for the synthetic release tag
  SAFELIBS_DIST_DIR       directory holding *.deb files to publish
  SAFELIBS_VALIDATOR_DIR  validator checkout (reads repositories.yml)
  SAFELIBS_LOCK_PATH      output path for the generated lock JSON
  SAFELIBS_OVERRIDE_ROOT  output root for <root>/<library>/*.deb layout

Exit codes:
  0  lock written, override populated
  2  library is not in the validator manifest (caller should skip cleanly)
  1  any other failure
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

LIBRARY_NAME_RE = re.compile(r"^[a-z0-9][a-z0-9_-]*$")
SHA256_HEX_RE = re.compile(r"^[0-9a-f]{40,}$")


def fail(message: str, code: int = 1) -> "None":
    print(f"build_port_lock: {message}", file=sys.stderr)
    sys.exit(code)


def required_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        fail(f"missing required environment variable: {name}")
    return value


def parse_canonical_packages(repositories_yml: Path, library: str) -> list[str] | None:
    """Return canonical apt_packages for `library`, or None if absent.

    Handcrafted YAML reader: validator manifests use a fixed shape (top-level
    `libraries:` list with `- name: <id>` and `apt_packages: [..]`). Avoiding
    PyYAML keeps the script dependency-free on a fresh ubuntu-latest runner.
    """
    in_libraries = False
    current_name: str | None = None
    current_packages: list[str] | None = None
    text = repositories_yml.read_text()
    for raw_line in text.splitlines():
        if raw_line.startswith("libraries:"):
            in_libraries = True
            continue
        if not in_libraries:
            continue
        if raw_line and not raw_line.startswith((" ", "\t")):
            break
        stripped = raw_line.strip()
        if stripped.startswith("- name:"):
            if current_name == library and current_packages is not None:
                return current_packages
            current_name = stripped.split(":", 1)[1].strip()
            current_packages = None
            continue
        if stripped.startswith("apt_packages:"):
            current_packages = []
            continue
        if current_packages is not None and stripped.startswith("- "):
            package = stripped[2:].strip().strip('"').strip("'")
            current_packages.append(package)
            continue
    if current_name == library and current_packages is not None:
        return current_packages
    return None


def dpkg_field(deb_path: Path, field_name: str) -> str:
    completed = subprocess.run(
        ["dpkg-deb", "--field", str(deb_path), field_name],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    library = required_env("SAFELIBS_LIBRARY")
    commit_sha = required_env("SAFELIBS_COMMIT_SHA")
    dist_dir = Path(required_env("SAFELIBS_DIST_DIR"))
    validator_dir = Path(required_env("SAFELIBS_VALIDATOR_DIR"))
    lock_path = Path(required_env("SAFELIBS_LOCK_PATH"))
    override_root = Path(required_env("SAFELIBS_OVERRIDE_ROOT"))

    if not LIBRARY_NAME_RE.match(library):
        fail(f"invalid SAFELIBS_LIBRARY: {library!r}")
    if len(commit_sha) < 12 or not all(c in "0123456789abcdef" for c in commit_sha.lower()):
        fail(f"SAFELIBS_COMMIT_SHA must be a hex commit id, got {commit_sha!r}")

    repositories_yml = validator_dir / "repositories.yml"
    if not repositories_yml.is_file():
        fail(f"validator manifest not found: {repositories_yml}")

    canonical_packages = parse_canonical_packages(repositories_yml, library)
    if canonical_packages is None:
        return 2
    if not canonical_packages:
        fail(f"library {library} has empty apt_packages in validator manifest")

    debs = sorted(dist_dir.glob("*.deb"))
    if not debs:
        fail(f"no .deb files under {dist_dir}")

    debs_by_package: dict[str, dict[str, object]] = {}
    library_override = override_root / library
    library_override.mkdir(parents=True, exist_ok=True)
    for stale in library_override.glob("*"):
        if stale.is_file() or stale.is_symlink():
            stale.unlink()

    for deb_path in debs:
        package = dpkg_field(deb_path, "Package")
        if not package:
            fail(f"{deb_path.name} has empty Package field")
        if package not in canonical_packages:
            print(
                f"build_port_lock: skipping {deb_path.name}: "
                f"Package {package!r} is not canonical for {library}"
            )
            continue
        if package in debs_by_package:
            fail(f"multiple dist/*.deb claim package {package}: existing "
                 f"{debs_by_package[package]['filename']!r}, new {deb_path.name!r}")
        architecture = dpkg_field(deb_path, "Architecture")
        if architecture not in {"amd64", "all"}:
            fail(f"{deb_path.name}: validator only accepts amd64 or all, got {architecture!r}")
        size = deb_path.stat().st_size
        sha256 = file_sha256(deb_path)
        target = library_override / deb_path.name
        shutil.copyfile(deb_path, target)
        debs_by_package[package] = {
            "package": package,
            "filename": deb_path.name,
            "architecture": architecture,
            "sha256": sha256,
            "size": size,
        }

    if not debs_by_package:
        fail(f"none of the dist/*.deb files match canonical packages for {library}: "
             f"expected one of {canonical_packages}")

    unported = [pkg for pkg in canonical_packages if pkg not in debs_by_package]
    ordered_debs = [debs_by_package[pkg] for pkg in canonical_packages if pkg in debs_by_package]

    lock = {
        "schema_version": 1,
        "mode": "port-04-test",
        "libraries": [
            {
                "library": library,
                "repository": f"safelibs/port-{library}",
                "url": f"https://github.com/safelibs/port-{library}",
                "tag_ref": f"refs/tags/build-{commit_sha[:12]}",
                "commit": commit_sha,
                "release_tag": f"build-{commit_sha[:12]}",
                "debs": ordered_debs,
                "unported_original_packages": unported,
            }
        ],
    }

    lock_path.parent.mkdir(parents=True, exist_ok=True)
    lock_path.write_text(json.dumps(lock, indent=2) + "\n")
    print(
        f"build_port_lock: wrote {lock_path} with "
        f"{len(ordered_debs)} ported / {len(unported)} unported package(s)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
