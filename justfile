# Recipes for managing the vendored slang static libraries.
#
# `main` never carries the static libs. `fetch-static` materializes them into
# a gitignored directory for development and CI; `release` commits them on a
# tag-only commit that never lands on `main`. See plans/00_static_build.md.

slang_version := "2026.13.1"
slang_release := "v" + slang_version + "-static"
slang_repo := "https://github.com/Giesch/slang"
vendor := "slang-sys/vendor"
vendor_local := "slang-sys/vendor-local"
platforms := "linux-x86_64 macos-aarch64 windows-x86_64"

_default:
    @just --list

# download, verify, and extract the pinned slang static release (gitignored)
fetch-static:
    #!/usr/bin/env sh
    set -eu
    mkdir -p "{{ vendor_local }}"
    for platform in {{ platforms }}; do
        asset="slang-static-{{ slang_version }}-${platform}.tar.xz"
        [ -f "{{ vendor_local }}/${asset}" ] || \
            curl -fL --retry 3 -o "{{ vendor_local }}/${asset}" \
                "{{ slang_repo }}/releases/download/{{ slang_release }}/${asset}"
    done
    cd "{{ vendor_local }}"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -c ../static-release.sha256
    else
        shasum -a 256 -c ../static-release.sha256
    fi
    for platform in {{ platforms }}; do
        rm -rf "${platform}"
        mkdir -p "${platform}"
        tar -xf "slang-static-{{ slang_version }}-${platform}.tar.xz" \
            --strip-components 1 -C "${platform}"
    done

# regenerate slang-sys/src/bindings.rs from the fetched or vendored headers
regen-bindings:
    cargo build -p shader-slang-sys --no-default-features --features static,regenerate-bindings

# vendor the static libs on a tag-only release commit and push the tag
release tag:
    #!/usr/bin/env sh
    set -eu
    [ -z "$(git status --porcelain)" ] || { echo "error: working tree not clean" >&2; exit 1; }
    [ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || { echo "error: must be on main" >&2; exit 1; }
    git fetch origin main
    [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "error: main is not up to date with origin/main" >&2; exit 1; }
    just fetch-static
    mkdir -p "{{ vendor }}"
    for platform in {{ platforms }}; do
        rm -rf "{{ vendor }}/${platform}"
        mv "{{ vendor_local }}/${platform}" "{{ vendor }}/${platform}"
    done
    git add "{{ vendor }}"
    git commit -m "Release {{ tag }}: vendor slang {{ slang_version }} static libs"
    git tag "{{ tag }}"
    git push origin "refs/tags/{{ tag }}"
    git reset --hard origin/main
