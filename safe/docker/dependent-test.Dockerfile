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
      runtime_pkg=(/tmp/liblzma-safe-packages/liblzma5_*.deb); \
      dev_pkg=(/tmp/liblzma-safe-packages/liblzma-dev_*.deb); \
      [[ -f "${runtime_pkg[0]}" ]] || { printf 'missing staged liblzma5 package\n' >&2; exit 1; }; \
      [[ -f "${dev_pkg[0]}" ]] || { printf 'missing staged liblzma-dev package\n' >&2; exit 1; }; \
      for dpkg_cfg in /etc/dpkg/dpkg.cfg.d/docker /etc/dpkg/dpkg.cfg.d/excludes; do \
        if [[ -f "$dpkg_cfg" ]]; then \
          mv "$dpkg_cfg" "$dpkg_cfg.disabled"; \
        fi; \
      done; \
      dpkg -i "${runtime_pkg[0]}" "${dev_pkg[0]}"; \
      apt-get install -f -y --no-install-recommends; \
      ldconfig; \
    fi \
 && rm -rf /tmp/liblzma-safe-packages /var/lib/apt/lists/*
