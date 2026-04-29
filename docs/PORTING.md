# Porting Guide

Use this checklist when creating a new `safelibs/port-*` repository from this template. Read [AGENTS.md](../AGENTS.md) first — it defines the script contracts every step here assumes.

## Port Checklist

1. Create a new `safelibs/port-*` repository from this template.
2. Fill `original/` with the upstream source snapshot, or precise import/build instructions.
3. Fill `safe/` with the safe implementation and any files that should be packaged.
4. Replace the placeholder data in `all_cves.json`, `dependents.json`, and `relevant_cves.json`.
5. Add upstream regression tests under `tests/upstream/`, or replace `scripts/run-upstream-tests.sh` with a port-specific runner that installs `dist/*.deb` and invokes the upstream test harness.
6. Add port-authored tests under `tests/port/`, or replace `scripts/run-port-tests.sh` with a port-specific runner.
7. Update `packaging/package.env` — at minimum set `SAFELIBS_LIBRARY` to the validator manifest identifier; update the `DEB_*` fields if you keep the reference `scripts/build-debs.sh`.
8. Override `scripts/install-build-deps.sh` and `scripts/build-debs.sh` whenever the template defaults do not fit your build (typical for ports that use `dpkg-buildpackage`, `cargo`, `cmake`, or custom build scripts).
9. Run local validation, tests, and package build commands.
10. Push to `main`. Inspect the latest CI run and the `build-<short-sha>` GitHub Release.

## Consuming Existing Artifacts

Future ports may already have checked-in source snapshots, CVE data, dependent inventories, or test harnesses prepared by earlier workflow phases. Treat those artifacts as inputs:

- If `original/` already contains the needed upstream snapshot or import instructions, use it as the source of truth.
- If `all_cves.json`, `dependents.json`, or `relevant_cves.json` already contains prepared data, preserve useful entries and edit in place.
- If `tests/upstream/`, `tests/port/`, or any of the `scripts/*.sh` already contains a working harness, keep it and adapt it to the final layout.
- If an artifact is missing, incomplete, or known to be stale, document the correction in the commit that updates it.

## Source Layout

Put the upstream or original implementation under `original/`. A source snapshot is preferred when it is small enough and legally appropriate to check in. If the source should not be committed directly, keep deterministic import instructions in `original/README.md` or adjacent scripts.

Put the safe implementation under `safe/`. The reference `scripts/build-debs.sh` copies files from `safe/` into `DEB_INSTALL_PREFIX` while excluding `.git`, `build`, `dist`, `.gitkeep`, and `README.md`. Most real ports replace that script with `dpkg-buildpackage` rooted in `safe/debian/`; in that case the `DEB_*` fields in `packaging/package.env` can stay at the template defaults but `SAFELIBS_LIBRARY` is still required.

## CVE And Dependent Data

Replace the placeholders with port-specific data:

- `all_cves.json`: full CVE inventory considered for the original package or project.
- `relevant_cves.json`: subset of CVEs relevant to the safe implementation, including the selection criteria.
- `dependents.json`: dependent packages, projects, or applications used to evaluate compatibility and risk.

Keep the files valid JSON. `scripts/check-layout.sh` runs `python3 -m json.tool` on each.

## Tests

The CI hook sequence runs three test scripts in order: `run-upstream-tests.sh`, `run-port-tests.sh`, `run-validation-tests.sh`. The first two delegate to `scripts/run-tests.sh`, which executes every `*.sh` under `tests/upstream/` or `tests/port/`. The third runs the [safelibs/validator](https://github.com/safelibs/validator) matrix against the just-built `.deb`s.

- Upstream regression tests prove API/ABI compatibility with what real consumers expect; usually they install `dist/*.deb` into a chroot or container and invoke the upstream harness.
- Port-authored tests cover whatever the safe implementation uniquely needs: unit tests, ABI checks, fuzzing, differential tests.
- Validation tests run automatically once `SAFELIBS_LIBRARY` matches an entry in the validator manifest. A port that is not yet listed in the validator (or the template itself) skips this hook cleanly.

The placeholder upstream and port harnesses exit successfully when no tests exist. That is only for template bootstrap; a real port should provide meaningful tests.

## Packaging

Update `packaging/package.env`:

- `SAFELIBS_LIBRARY`: validator manifest identifier and `safelibs/port-<library>` suffix.
- `DEB_PACKAGE`: package name, e.g. `safelibs-port-example`.
- `DEB_VERSION`: base version; the reference builder appends `+git.<commit-sha>`.
- `DEB_ARCHITECTURE`: Debian architecture or `auto`.
- `DEB_MAINTAINER`: real maintainer contact.
- `DEB_SECTION`: package section, usually `libs`.
- `DEB_PRIORITY`: package priority, usually `optional`.
- `DEB_DESCRIPTION`: short package description.
- `DEB_INSTALL_PREFIX`: absolute install path for copied `safe/` files.
- `DEB_DEPENDS`: comma-separated dependencies or empty string.

Build locally before pushing:

```sh
rm -rf build dist
bash scripts/build-debs.sh
```

Resulting `.deb`(s) land in `dist/`.

## Local Verification

Run the full hook sequence the same way CI runs it:

```sh
bash scripts/install-build-deps.sh
bash scripts/check-layout.sh
rm -rf build dist
bash scripts/build-debs.sh
bash scripts/run-upstream-tests.sh
bash scripts/run-port-tests.sh
bash scripts/run-validation-tests.sh
```

To reuse a local validator checkout (faster than re-cloning), set `SAFELIBS_VALIDATOR_DIR=/path/to/validator`. Fix any failure before pushing to `main`.

## Push And Inspect

Push completed port work to `main`. CI runs the hook sequence, uploads every `dist/*.deb` as an Actions artifact, and creates or updates a `build-<short-sha>` GitHub Release.

After pushing, inspect:

- The latest run of `.github/workflows/ci-release.yml`.
- The uploaded `.deb` artifacts for the workflow run.
- The GitHub Release for `build-<short-sha>`.
