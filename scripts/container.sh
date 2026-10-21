#!/bin/sh
# Runs the build inside a container, so the host never needs the Tauri
# Linux prerequisites (WebKitGTK, gtk, librsvg, pkg-config, Rust) installed.
#
# Usage:
#   scripts/container.sh build            # build the toolchain image
#   scripts/container.sh make             # run scripts/package.sh inside it
#   scripts/container.sh test             # run cargo test --workspace
#   scripts/container.sh shell            # interactive shell in the image
#   scripts/container.sh release          # build + package the release archive
#   scripts/container.sh run <cmd...>     # run a command in the image
#
# podman is preferred (rootless, no daemon); docker is used when podman is
# missing. The image is tagged so the host can also `podman run` it directly.

set -eu
cd "$(dirname "$0")/.."

# Prefer podman, fall back to docker.
if command -v podman >/dev/null 2>&1; then
    container=podman
elif command -v docker >/dev/null 2>&1; then
    container=docker
else
    echo "scripts/container.sh: neither podman nor docker is installed." >&2
    exit 1
fi

image=tabletforge:builder

case "${1:-}" in
    build)
        # The image build has no host caches to share; buildkit mounts would
        # need pre-existing dirs, so the toolchain is installed fresh here.
        $container build -t "$image" -f Containerfile --target builder .
        ;;
    make)
        $container run --rm -t \
            -v "$(pwd):/workspace" \
            "$image" sh -c 'cargo build --release -p tabletforged -p tabletforge && ./scripts/package.sh'
        ;;
    test)
        $container run --rm -t \
            -v "$(pwd):/workspace" \
            "$image" cargo test --workspace
        ;;
    release)
        $container run --rm -t \
            -v "$(pwd):/workspace" \
            "$image" sh -c 'cargo build --release -p tabletforged -p tabletforge && ./scripts/package.sh'
        ;;
    shell)
        $container run --rm -ti \
            -v "$(pwd):/workspace" \
            "$image" /bin/bash
        ;;
    run)
        shift
        $container run --rm -t \
            -v "$(pwd):/workspace" \
            "$image" "$@"
        ;;
    *)
        echo "usage: $0 {build|make|test|shell|release|run <cmd...>}" >&2
        exit 1
        ;;
esac