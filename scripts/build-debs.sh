#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'build-debs: %s\n' "$*" >&2
  exit 1
}

repo_root() {
  local script_dir

  script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  if command -v git >/dev/null 2>&1 && git -C "$script_dir/.." rev-parse --show-toplevel >/dev/null 2>&1; then
    git -C "$script_dir/.." rev-parse --show-toplevel
  else
    cd -- "$script_dir/.." && pwd
  fi
}

trim_whitespace() {
  local value="$1"

  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

is_supported_package_var() {
  case "$1" in
    SAFELIBS_LIBRARY | DEB_PACKAGE | DEB_VERSION | DEB_ARCHITECTURE | DEB_MAINTAINER | DEB_SECTION | DEB_PRIORITY | DEB_DESCRIPTION | DEB_INSTALL_PREFIX | DEB_DEPENDS)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

validate_config_value() {
  local path="$1"
  local line_number="$2"
  local value="$3"

  if [[ "$value" == *'$('* || "$value" == *'`'* || "$value" == *'<('* || "$value" == *'>('* ]]; then
    fail "$path:$line_number contains command or process substitution"
  fi

  if [[ "$value" =~ ^\'[^\']*\'$ ]]; then
    return 0
  fi

  if [[ "$value" =~ ^\"([^\"\\]|\\.)*\"$ ]]; then
    if [[ "$value" == *'$'* ]]; then
      fail "$path:$line_number contains shell expansion in a double-quoted value"
    fi
    return 0
  fi

  if [[ "$value" =~ ^[A-Za-z0-9._:/+@%,=-]*$ ]]; then
    return 0
  fi

  fail "$path:$line_number has an unquoted value containing whitespace or shell metacharacters"
}

validate_package_config_file() {
  local config_path="$1"
  local relative_path="$2"
  local line
  local line_number=0
  local trimmed
  local name
  local value

  [[ -f "$config_path" ]] || fail "missing package config: $relative_path"

  while IFS= read -r line || [[ -n "$line" ]]; do
    line_number=$((line_number + 1))
    trimmed="$(trim_whitespace "$line")"

    if [[ -z "$trimmed" || "${trimmed:0:1}" == "#" ]]; then
      continue
    fi

    if [[ "$trimmed" == export || "$trimmed" == export[[:space:]]* ]]; then
      fail "$relative_path:$line_number export statements are not supported"
    fi

    if [[ "$trimmed" =~ ^([A-Za-z_][A-Za-z0-9_]*)=(.*)$ ]]; then
      name="${BASH_REMATCH[1]}"
      value="${BASH_REMATCH[2]}"
    else
      fail "$relative_path:$line_number is not a supported assignment"
    fi

    is_supported_package_var "$name" || fail "$relative_path:$line_number uses unsupported variable: $name"
    validate_config_value "$relative_path" "$line_number" "$value"
  done < "$config_path"
}

load_package_config() {
  local root
  local config_path
  local var
  local required_vars=(
    SAFELIBS_LIBRARY
    DEB_PACKAGE
    DEB_VERSION
    DEB_ARCHITECTURE
    DEB_MAINTAINER
    DEB_SECTION
    DEB_PRIORITY
    DEB_DESCRIPTION
    DEB_INSTALL_PREFIX
    DEB_DEPENDS
  )

  root="$(repo_root)"
  config_path="$root/packaging/package.env"

  validate_package_config_file "$config_path" "packaging/package.env"

  unset SAFELIBS_LIBRARY DEB_PACKAGE DEB_VERSION DEB_ARCHITECTURE DEB_MAINTAINER DEB_SECTION DEB_PRIORITY DEB_DESCRIPTION DEB_INSTALL_PREFIX DEB_DEPENDS
  # shellcheck source=/dev/null
  source "$config_path"

  for var in "${required_vars[@]}"; do
    if [[ ! ${!var+x} ]]; then
      fail "missing required package metadata: $var"
    fi
  done

  for var in SAFELIBS_LIBRARY DEB_PACKAGE DEB_VERSION DEB_ARCHITECTURE DEB_MAINTAINER DEB_SECTION DEB_PRIORITY DEB_DESCRIPTION DEB_INSTALL_PREFIX; do
    if [[ -z "${!var}" ]]; then
      fail "package metadata must not be empty: $var"
    fi
  done

  [[ "$SAFELIBS_LIBRARY" =~ ^[a-z0-9][a-z0-9_-]*$ ]] || fail "SAFELIBS_LIBRARY is not a valid library identifier: $SAFELIBS_LIBRARY"
  [[ "$DEB_PACKAGE" =~ ^[a-z0-9][a-z0-9+.-]*$ ]] || fail "DEB_PACKAGE is not a valid Debian package name: $DEB_PACKAGE"
  [[ "$DEB_INSTALL_PREFIX" == /* ]] || fail "DEB_INSTALL_PREFIX must be an absolute path"
  [[ "$DEB_INSTALL_PREFIX" != "/" ]] || fail "DEB_INSTALL_PREFIX must not be /"
}

current_commit_id() {
  local root

  if [[ -n "${SAFELIBS_COMMIT_SHA:-}" ]]; then
    printf '%s\n' "$SAFELIBS_COMMIT_SHA"
    return 0
  fi

  root="$(repo_root)"
  if command -v git >/dev/null 2>&1 && git -C "$root" rev-parse HEAD >/dev/null 2>&1; then
    git -C "$root" rev-parse HEAD
    return 0
  fi

  if [[ -n "${GITHUB_SHA:-}" ]]; then
    printf '%s\n' "$GITHUB_SHA"
    return 0
  fi

  printf 'local\n'
}

package_version() {
  local commit_id
  local version

  commit_id="$(current_commit_id)"
  version="${DEB_VERSION}+git.${commit_id}"

  [[ -n "$DEB_VERSION" ]] || fail "DEB_VERSION must not be empty"
  [[ -n "$commit_id" ]] || fail "commit id must not be empty"
  if [[ "$version" =~ [[:space:]] ]]; then
    fail "package version must not contain whitespace: $version"
  fi

  printf '%s\n' "$version"
}

resolve_architecture() {
  if [[ "$DEB_ARCHITECTURE" == "auto" ]]; then
    command -v dpkg >/dev/null 2>&1 || fail "dpkg is required when DEB_ARCHITECTURE=auto"
    dpkg --print-architecture
    return 0
  fi

  [[ -n "$DEB_ARCHITECTURE" ]] || fail "DEB_ARCHITECTURE must not be empty"
  if [[ "$DEB_ARCHITECTURE" =~ [[:space:]] ]]; then
    fail "DEB_ARCHITECTURE must not contain whitespace: $DEB_ARCHITECTURE"
  fi

  printf '%s\n' "$DEB_ARCHITECTURE"
}

prepare_package_tree() {
  local root

  root="$(repo_root)"
  package_root="$root/build/deb/$DEB_PACKAGE"

  rm -rf -- "$package_root" "$root/dist"
  mkdir -p -- "$package_root/DEBIAN" "$root/dist"
  chmod 0755 "$package_root/DEBIAN"
}

copy_safe_payload() {
  local root
  local safe_dir
  local install_dir
  local manifest

  root="$(repo_root)"
  safe_dir="$root/safe"
  install_dir="$package_root$DEB_INSTALL_PREFIX"

  [[ -d "$safe_dir" ]] || fail "missing safe implementation directory: safe"

  mkdir -p -- "$install_dir"
  manifest="$(mktemp)"

  (
    cd "$safe_dir"
    find . -mindepth 1 \
      \( -name .git -o -name build -o -name dist -o -name .gitkeep -o -path ./README.md \) -prune \
      -o -print0 > "$manifest"
  )

  if [[ -s "$manifest" ]]; then
    (
      cd "$safe_dir"
      tar --null -cf - --files-from "$manifest"
    ) | (
      cd "$install_dir"
      tar -xf -
    )
  fi

  rm -f -- "$manifest"

  if ! find "$install_dir" -mindepth 1 \( -type f -o -type l \) -print -quit | grep -q .; then
    printf 'No packageable safe artifacts were present when this package was built.\n' > "$install_dir/SAFE_LIBS_PLACEHOLDER.txt"
    chmod 0644 "$install_dir/SAFE_LIBS_PLACEHOLDER.txt"
  fi
}

write_control_file() {
  local control_file

  control_file="$package_root/DEBIAN/control"

  {
    printf 'Package: %s\n' "$DEB_PACKAGE"
    printf 'Version: %s\n' "$version"
    printf 'Section: %s\n' "$DEB_SECTION"
    printf 'Priority: %s\n' "$DEB_PRIORITY"
    printf 'Architecture: %s\n' "$arch"
    printf 'Maintainer: %s\n' "$DEB_MAINTAINER"
    if [[ -n "$DEB_DEPENDS" ]]; then
      printf 'Depends: %s\n' "$DEB_DEPENDS"
    fi
    printf 'Description: %s\n' "$DEB_DESCRIPTION"
  } > "$control_file"

  chmod 0644 "$control_file"
}

build_deb() {
  local root
  local output_path

  root="$(repo_root)"
  output_path="$root/dist/${DEB_PACKAGE}_${version}_${arch}.deb"

  command -v dpkg-deb >/dev/null 2>&1 || fail "dpkg-deb is required to build Debian packages"
  dpkg-deb --build --root-owner-group "$package_root" "$output_path"
}

main() {
  load_package_config
  version="$(package_version)"
  arch="$(resolve_architecture)"
  prepare_package_tree
  copy_safe_payload
  write_control_file
  build_deb
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
