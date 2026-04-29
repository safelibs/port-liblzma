# port-liblzma

SafeLibs port of `liblzma` for Ubuntu 24.04. Built via `dpkg-buildpackage` rooted in `safe/debian/`.

This repository follows the [`safelibs/port-template`](https://github.com/safelibs/port-template) contract. See [`AGENTS.md`](AGENTS.md) for the canonical layout, hook-script contracts, and CI sequence.

## Local Build

```sh
bash scripts/install-build-deps.sh
bash scripts/check-layout.sh
rm -rf build dist
bash scripts/build-debs.sh
```

`.deb` artifacts land in `dist/`.
