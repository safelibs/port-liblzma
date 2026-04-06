FROM ubuntu:24.04

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ARG DEBIAN_FRONTEND=noninteractive
ARG LIBLZMA_IMPLEMENTATION=original

COPY packages/ /tmp/liblzma-safe-packages/

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      apt \
      apt-utils \
      binutils \
      build-essential \
      ca-certificates \
      dpkg-dev \
      gdb \
      kmod \
      libarchive-dev \
      libarchive-tools \
      libboost-iostreams-dev \
      libtiff-dev \
      libxml2-dev \
      libxml2-utils \
      mariadb-client \
      mariadb-plugin-provider-lzma \
      mariadb-server \
      pkg-config \
      python3 \
      python3.12 \
      squashfs-tools \
      xz-utils \
 && if [[ "$LIBLZMA_IMPLEMENTATION" == "safe" ]]; then \
      runtime_pkg=/tmp/liblzma-safe-packages/liblzma5.deb; \
      dev_pkg=/tmp/liblzma-safe-packages/liblzma-dev.deb; \
      [[ -f "$runtime_pkg" ]] || { printf 'missing staged liblzma5 package\n' >&2; exit 1; }; \
      [[ -f "$dev_pkg" ]] || { printf 'missing staged liblzma-dev package\n' >&2; exit 1; }; \
      runtime_version="$(dpkg-deb -f "$runtime_pkg" Version)"; \
      dev_version="$(dpkg-deb -f "$dev_pkg" Version)"; \
      for dpkg_cfg in /etc/dpkg/dpkg.cfg.d/docker /etc/dpkg/dpkg.cfg.d/excludes; do \
        if [[ -f "$dpkg_cfg" ]]; then \
          mv "$dpkg_cfg" "$dpkg_cfg.disabled"; \
        fi; \
      done; \
      dpkg -i "$runtime_pkg" "$dev_pkg"; \
      apt-get install -f -y --no-install-recommends; \
      [[ "$(dpkg-query -W -f='${Version}' liblzma5)" == "$runtime_version" ]] || { printf 'liblzma5 version mismatch after safe package install\n' >&2; exit 1; }; \
      [[ "$(dpkg-query -W -f='${Version}' liblzma-dev)" == "$dev_version" ]] || { printf 'liblzma-dev version mismatch after safe package install\n' >&2; exit 1; }; \
      multiarch="$(dpkg-architecture -qDEB_HOST_MULTIARCH)"; \
      expected_runtime="$(dpkg -L liblzma5 | grep -E '/liblzma\.so\.5\.4\.5$' | head -n 1)"; \
      [[ -n "$expected_runtime" ]] || { printf 'failed to locate packaged liblzma.so.5.4.5\n' >&2; exit 1; }; \
      [[ "$(readlink -f "/usr/lib/${multiarch}/liblzma.so.5")" == "$expected_runtime" ]] || { printf 'safe image runtime path did not resolve to the packaged liblzma5 payload\n' >&2; exit 1; }; \
      [[ -f "/usr/lib/${multiarch}/pkgconfig/liblzma.pc" ]] || { printf 'missing packaged liblzma.pc in safe image\n' >&2; exit 1; }; \
      ldconfig; \
    fi \
 && rm -rf /tmp/liblzma-safe-packages /var/lib/apt/lists/*
