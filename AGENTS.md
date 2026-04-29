# AGENTS.md

Canonical contract for `safelibs/port-template` and every `safelibs/port-*` repository created from it. Read this before adding a new port, modifying a script under `scripts/`, or editing the workflow file.

## Why This Document Exists

Earlier versions of the template defined CI as a fixed sequence of inline workflow steps. Each port that diverged from the default needed a per-port workflow override, and a separate generator in `safelibs/apt` rendered those overrides from a central config. That bifurcation made it brittle to evolve either side.

The current shape moves the divergence into hook scripts under `scripts/`. The workflow runs a fixed sequence of those hooks. Ports own their own hook contents; the template ships sensible defaults. There is no generator and no central config — each port is self-describing.

## CI Pipeline

`.github/workflows/ci-release.yml` runs the following sequence on every push to `main` and every manual `workflow_dispatch`:

| # | Step | Script | Template default |
| - | ---- | ------ | ---------------- |
| 1 | Install build dependencies | `scripts/install-build-deps.sh` | No-op |
| 2 | Check repository layout | `scripts/check-layout.sh` | Lint required files / executable bits |
| 3 | Build .deb artifacts | `scripts/build-debs.sh` | Reference build from `packaging/package.env` + `safe/` |
| 4 | Run upstream tests | `scripts/run-upstream-tests.sh` | Run every `*.sh` under `tests/upstream/` |
| 5 | Run port tests | `scripts/run-port-tests.sh` | Run every `*.sh` under `tests/port/` |
| 6 | Run validation tests | `scripts/run-validation-tests.sh` | Clone `safelibs/validator`, run `port-04-test` mode against `dist/*.deb` |
| 7 | Upload `dist/*.deb` | (workflow) | One GitHub Actions artifact per run |
| 8 | Publish release | (workflow) | `build-<short-sha>` GitHub Release with every `dist/*.deb` |

Steps 1–6 are hooks. Steps 7–8 are workflow-owned and ports do not customize them.

## Script Contracts

Each hook script is invoked with `bash <script>` from the repository root. `set -euo pipefail` is the convention. Scripts must succeed (exit 0) on the happy path; non-zero is a CI failure.

### `scripts/install-build-deps.sh`

Install everything the build and tests need that is not preinstalled on `ubuntu-latest`:

- apt packages (compilers, dev libraries, packaging tools)
- language toolchains (rustup, cargo, custom Python venvs)
- any other system-level setup

May invoke `sudo`. Must be idempotent — reruns on a warm runner must succeed. Template default is a no-op because the reference build only needs `dpkg-deb`, which is preinstalled.

### `scripts/check-layout.sh`

Lint the repository against the template contract: required files exist, scripts are executable, JSON inventories parse, `.gitattributes` carries the expected entries, and `packaging/package.env` is well-formed. A port can extend this with port-specific invariants but must keep the baseline checks.

### `scripts/build-debs.sh`

Produce one or more Debian package files under `dist/`. There is no upper bound on the number of `.deb` files — most ports emit a handful (runtime, dev, tools, docs).

The reference implementation copies `safe/` into `DEB_INSTALL_PREFIX` from `packaging/package.env` and emits a single `.deb` via `dpkg-deb --build`. Real ports usually replace this with `dpkg-buildpackage -us -uc -b` rooted in `safe/debian/`, or with a port-owned build script (`bash safe/scripts/build-deb.sh`, `cargo run -p xtask -- package-deb`, etc.).

`SAFELIBS_COMMIT_SHA` is set in CI; a build script that stamps versions should consume it via that variable.

### `scripts/run-upstream-tests.sh`

Run the upstream library's regression suite against the just-built safe `.deb`s. The name was chosen to be unambiguous: it runs *upstream's* suite, not tests of upstream's behavior. The intent is to prove the safe implementation is API/ABI-compatible with what real consumers of the upstream library expect.

The template default scans `tests/upstream/*.sh`. A real port typically replaces this with a script that installs `dist/*.deb` into a chroot or container and invokes the upstream test harness via `make check`, `meson test`, or similar.

This script may need the artifacts produced by `build-debs.sh`. CI runs it after the build; local invocations must run the build first.

### `scripts/run-port-tests.sh`

Run port-authored tests for the safe implementation: unit tests, ABI checks, fuzzing harnesses, differential tests against upstream. The template default scans `tests/port/*.sh`. Ports replace it with whatever framework fits their language and toolchain.

### `scripts/run-validation-tests.sh`

Run the [safelibs/validator](https://github.com/safelibs/validator) test matrix in `port-04-test` mode against `dist/*.deb`.

Inputs (mostly read from `packaging/package.env`):

- `SAFELIBS_LIBRARY` — must match a `name:` entry in the validator's `repositories.yml`.
- `dist/*.deb` — produced by the build hook.
- `SAFELIBS_COMMIT_SHA` — used as the synthetic release tag.

Optional environment overrides:

- `SAFELIBS_VALIDATOR_DIR` — path to an existing validator checkout. When unset, the script clones `https://github.com/safelibs/validator` into `.work/validator`.
- `SAFELIBS_VALIDATOR_REF` — git ref to clone (default `main`).
- `SAFELIBS_VALIDATOR_REPO` — git remote (default `https://github.com/safelibs/validator`).
- `SAFELIBS_RECORD_CASTS` — non-empty enables `--record-casts`.

Behavior:

1. Reads canonical `apt_packages` for `SAFELIBS_LIBRARY` from the validator manifest.
2. Inspects every `dist/*.deb`, matching them by `dpkg-deb --field Package` against the canonical list. Non-canonical extras are ignored. Canonical packages with no matching deb become `unported_original_packages`.
3. Synthesizes a `port-04-test` deb lock JSON file with the matching debs, sha256s, sizes, and the synthesized `release_tag = build-<commit[:12]>`.
4. Lays out `<override-deb-root>/<library>/<filename>.deb`.
5. Invokes `bash <validator>/test.sh --library <SAFELIBS_LIBRARY> --mode port-04-test --override-deb-root ... --port-deb-lock ... --artifact-root ...`.

Soft skip: a library that has no entry in the validator manifest (the template itself, ports still being authored) returns a skip and the script exits 0. This is the only acceptable success without a real validator run.

Hard failures: missing `dist/*.deb`, no canonical packages matched, mismatch between dist debs and canonical packages, validator clone failure, validator matrix failure.

## Repository Layout

Required directories: `original/`, `safe/`, `packaging/`, `tests/upstream/`, `tests/port/`, `scripts/`.

Required files: `.github/workflows/ci-release.yml`, `.gitattributes`, `README.md`, `AGENTS.md`, `CLAUDE.md`, `all_cves.json`, `dependents.json`, `relevant_cves.json`, `packaging/package.env`.

Required executable scripts: `scripts/build-debs.sh`, `scripts/check-layout.sh`, `scripts/install-build-deps.sh`, `scripts/run-port-tests.sh`, `scripts/run-tests.sh`, `scripts/run-upstream-tests.sh`, `scripts/run-validation-tests.sh`.

`scripts/check-layout.sh` is the source of truth for the layout. When you change required files or scripts, update it in the same commit.

`docs/` (with the template's `PORTING.md` and `PUBLISHING.md` meta-guides) is intentionally **not** required of port repos — those documents describe how to *create* a port from the template and have no role inside an already-created port.

## `packaging/package.env`

The only field every port must set:

- `SAFELIBS_LIBRARY` — validator manifest identifier; must equal the repo name suffix (`safelibs/port-<SAFELIBS_LIBRARY>`).

The template's reference `scripts/build-debs.sh` *also* consumes a set of `DEB_*` fields (`DEB_PACKAGE`, `DEB_VERSION`, `DEB_ARCHITECTURE`, `DEB_MAINTAINER`, `DEB_SECTION`, `DEB_PRIORITY`, `DEB_DESCRIPTION`, `DEB_INSTALL_PREFIX`, `DEB_DEPENDS`) for its self-contained payload-copy build. Real ports override `build-debs.sh` (typically with `dpkg-buildpackage` rooted in `safe/debian/`) and can drop the `DEB_*` fields from `packaging/package.env` entirely.

## `scripts/lib/build-deb-common.sh`

A small bash library that ports overriding `build-debs.sh` may source. It provides:

- `prepare_rust_env` — source `~/.cargo/env`, prepend `~/.cargo/bin` to `PATH`.
- `prepare_dist_dir` — recreate `<repo>/dist` empty.
- `stamp_safelibs_changelog` — rewrite `debian/changelog` to version `<upstream>+safelibs<commit-epoch>`. Honors `SAFELIBS_COMMIT_SHA` when CI sets it.
- `build_with_dpkg_buildpackage` — run `mk-build-deps -i` + `dpkg-buildpackage -us -uc -b` and copy `../*.deb` into `<repo>/dist`.

Most port `build-debs.sh` scripts collapse to ~15 lines after sourcing this helper.

## Toolchain auto-detection

The template's default `scripts/install-build-deps.sh` reads `safe/rust-toolchain.toml`. When a `[toolchain] channel = "X"` line is present, X is installed as the rustup default for the build step. With no toolchain file, the script installs `stable`. Set `SAFELIBS_RUST_TOOLCHAIN` in the environment to override the file.

## Host-path lint

`scripts/check-layout.sh` scans `.cargo/config.toml`, `safe/.cargo/config.toml`, `safe/debian/rules`, `safe/Cargo.toml`, and every shell/TOML/JSON/Makefile under `safe/tools/` for hardcoded `/home/<user>/safelibs/port-*` paths that leak from local development. CI fails before the build step if any are found.

## When To Edit What

- **Adding/changing a port-specific build step:** edit the relevant hook script in your port repo. Do not edit the workflow file.
- **Changing the workflow shape (new step order, new global env, new artifact policy):** edit the template's `.github/workflows/ci-release.yml` and the corresponding section in this file. Sync changes back into `safelibs/port-*` repos by hand or by re-templating.
- **Adding a new required file or directory:** update `scripts/check-layout.sh`, this file, and `README.md` in the same commit.
- **Renaming a hook script:** update `scripts/check-layout.sh`, `.github/workflows/ci-release.yml`, this file, and `README.md` in the same commit.

## Non-Goals

- The template is **not** a generator. There is no central config that produces per-port workflows. Every port owns its scripts.
- The template is **not** a runtime library. It only defines layout and CI shape.
- The template does **not** support cross-port dependencies. A port hook may not assume another port has already run.
