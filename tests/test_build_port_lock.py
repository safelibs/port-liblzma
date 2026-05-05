"""Smoke test for scripts/lib/build_port_lock.py.

Exercises the YAML parser and the end-to-end lock synthesis against a
synthetic validator manifest + a one-line .deb produced via dpkg-deb.
The shape of the JSON the script writes is what the validator's
load_port_deb_lock() / _validate_port_lock_entry() consume; if any
field name or invariant moves, this test surfaces it locally instead
of waiting for a CI run on a real port.
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
LIB_PATH = REPO_ROOT / "scripts" / "lib" / "build_port_lock.py"


def load_module():
    spec = importlib.util.spec_from_file_location("build_port_lock", LIB_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ParseCanonicalPackagesTest(unittest.TestCase):
    def test_extracts_existing_library(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            manifest = Path(tmp) / "repositories.yml"
            manifest.write_text(
                "schema_version: 2\n"
                "libraries:\n"
                "  - name: cjson\n"
                "    apt_packages:\n"
                "      - libcjson1\n"
                "      - libcjson-dev\n"
                "  - name: libwebp\n"
                "    apt_packages:\n"
                "      - libwebp7\n"
                "      - libwebpdemux2\n"
            )
            self.assertEqual(
                module.parse_canonical_packages(manifest, "cjson"),
                ["libcjson1", "libcjson-dev"],
            )
            self.assertEqual(
                module.parse_canonical_packages(manifest, "libwebp"),
                ["libwebp7", "libwebpdemux2"],
            )

    def test_returns_none_for_missing_library(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            manifest = Path(tmp) / "repositories.yml"
            manifest.write_text(
                "libraries:\n"
                "  - name: cjson\n"
                "    apt_packages:\n"
                "      - libcjson1\n"
            )
            self.assertIsNone(module.parse_canonical_packages(manifest, "absent"))


class BuildPortLockEndToEndTest(unittest.TestCase):
    def setUp(self) -> None:
        if shutil.which("dpkg-deb") is None:
            self.skipTest("dpkg-deb not available")

    @staticmethod
    def make_minimal_deb(target: Path, *, package: str, architecture: str) -> None:
        with tempfile.TemporaryDirectory() as staging:
            root = Path(staging)
            (root / "DEBIAN").mkdir()
            (root / "DEBIAN" / "control").write_text(
                f"Package: {package}\n"
                "Version: 0.0.0\n"
                f"Architecture: {architecture}\n"
                "Maintainer: SafeLibs Tests <ci@safelibs.org>\n"
                "Description: smoke-test deb\n"
            )
            subprocess.run(
                ["dpkg-deb", "--build", "--root-owner-group", str(root), str(target)],
                check=True,
                capture_output=True,
            )

    def test_writes_lock_with_validator_required_fields(self) -> None:
        module = load_module()
        commit = "0123456789abcdef0123456789abcdef01234567"
        with tempfile.TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            validator_dir = tmp / "validator"
            validator_dir.mkdir()
            (validator_dir / "repositories.yml").write_text(
                "libraries:\n"
                "  - name: cjson\n"
                "    apt_packages:\n"
                "      - libcjson1\n"
                "      - libcjson-dev\n"
            )

            dist_dir = tmp / "dist"
            dist_dir.mkdir()
            self.make_minimal_deb(
                dist_dir / "libcjson1_0.0.0_amd64.deb",
                package="libcjson1",
                architecture="amd64",
            )
            self.make_minimal_deb(
                dist_dir / "libcjson-dev_0.0.0_amd64.deb",
                package="libcjson-dev",
                architecture="amd64",
            )

            lock_path = tmp / "port-deb-lock.json"
            override_root = tmp / "override-debs"
            override_root.mkdir()

            env = {
                "SAFELIBS_LIBRARY": "cjson",
                "SAFELIBS_COMMIT_SHA": commit,
                "SAFELIBS_DIST_DIR": str(dist_dir),
                "SAFELIBS_VALIDATOR_DIR": str(validator_dir),
                "SAFELIBS_LOCK_PATH": str(lock_path),
                "SAFELIBS_OVERRIDE_ROOT": str(override_root),
            }
            saved = {k: os.environ.get(k) for k in env}
            os.environ.update(env)
            try:
                self.assertEqual(module.main(), 0)
            finally:
                for k, v in saved.items():
                    if v is None:
                        os.environ.pop(k, None)
                    else:
                        os.environ[k] = v

            payload = json.loads(lock_path.read_text())
            self.assertEqual(payload["schema_version"], 1)
            self.assertEqual(payload["mode"], "port")

            self.assertEqual(len(payload["libraries"]), 1)
            entry = payload["libraries"][0]

            self.assertEqual(entry["library"], "cjson")
            self.assertEqual(entry["repository"], "safelibs/port-cjson")
            self.assertEqual(entry["commit"], commit)

            short = commit[:12]
            self.assertEqual(entry["release_tag"], f"build-{short}")
            # Validator now requires tag_ref == refs/tags/<release_tag>.
            self.assertEqual(entry["tag_ref"], f"refs/tags/build-{short}")

            packages = [d["package"] for d in entry["debs"]]
            self.assertEqual(packages, ["libcjson1", "libcjson-dev"])
            for deb in entry["debs"]:
                self.assertEqual(deb["architecture"], "amd64")
                self.assertEqual(len(deb["sha256"]), 64)
                self.assertTrue(all(c in "0123456789abcdef" for c in deb["sha256"]))
                self.assertGreater(deb["size"], 0)

            self.assertEqual(entry["unported_original_packages"], [])

            staged = sorted(p.name for p in (override_root / "cjson").iterdir())
            self.assertEqual(
                staged,
                ["libcjson-dev_0.0.0_amd64.deb", "libcjson1_0.0.0_amd64.deb"],
            )

    def test_skips_with_exit_2_when_library_absent(self) -> None:
        module = load_module()
        commit = "abcdef0123456789abcdef0123456789abcdef01"
        with tempfile.TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            validator_dir = tmp / "validator"
            validator_dir.mkdir()
            (validator_dir / "repositories.yml").write_text(
                "libraries:\n"
                "  - name: cjson\n"
                "    apt_packages:\n"
                "      - libcjson1\n"
            )
            dist_dir = tmp / "dist"
            dist_dir.mkdir()
            self.make_minimal_deb(
                dist_dir / "noop_0.0.0_amd64.deb",
                package="noop",
                architecture="amd64",
            )

            env = {
                "SAFELIBS_LIBRARY": "absent",
                "SAFELIBS_COMMIT_SHA": commit,
                "SAFELIBS_DIST_DIR": str(dist_dir),
                "SAFELIBS_VALIDATOR_DIR": str(validator_dir),
                "SAFELIBS_LOCK_PATH": str(tmp / "lock.json"),
                "SAFELIBS_OVERRIDE_ROOT": str(tmp / "override"),
            }
            saved = {k: os.environ.get(k) for k in env}
            os.environ.update(env)
            try:
                self.assertEqual(module.main(), 2)
            finally:
                for k, v in saved.items():
                    if v is None:
                        os.environ.pop(k, None)
                    else:
                        os.environ[k] = v


if __name__ == "__main__":
    unittest.main()
