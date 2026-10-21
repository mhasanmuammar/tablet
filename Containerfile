# syntax=docker/dockerfile:1

# TabletForge container build.
#
# Two stages:
#  builder  - the toolchain and every build dependency (GTK, WebKitGTK,
#             librsvg, Rust). Compiles the release binaries and the
#             installer archive, the same way scripts/package.sh does.
#  final    - a small image carrying the built artifacts and the runtime
#             libraries the binaries link against, in /output.
#
# podman reads Containerfile by default; docker reads Dockerfile. Both files
# are kept identical on purpose. Build the artifacts with:
#   scripts/container.sh release
# or build the toolchain image and compile incrementally with:
#   scripts/container.sh build && scripts/container.sh make

FROM ubuntu:22.04 AS builder

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/cargo \
    RUSTUP_HOME=/rustup \
    PATH=/cargo/bin:/rustup/bin:$PATH

# Build dependencies. Tauri's build script and the gtk/WebKit crates need
# pkg-config to find them; the rest is the toolchain and archives.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        file \
        pkg-config \
        libwebkit2gtk-4.1-dev \
        libgtk-3-dev \
        librsvg2-dev \
        libssl-dev \
        libxml2-dev \
    && rm -rf /var/lib/apt/lists/*

# Rust (stable), the same toolchain the README asks for.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /workspace
COPY . /workspace

# build (no --locked: lets cargo resolve available crate versions)
RUN cargo build --release -p tabletforged -p tabletforge \
    && ./scripts/package.sh

FROM ubuntu:22.04 AS final

ENV DEBIAN_FRONTEND=noninteractive

# Runtime shared libraries the binaries load (WebKitGTK pulls in gtk, glib,
# pango, cairo, ...). ca-certificates so https works if the app fetches.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libwebkit2gtk-4.1-0 \
        libgtk-3-0 \
        librsvg2-2 \
        libssl3 \
        libxml2 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /workspace/dist/ /output/
COPY --from=builder /workspace/target/release/tabletforged /output/bin/tabletforged
COPY --from=builder /workspace/target/release/tabletforge /output/bin/tabletforge

CMD ["sh", "-c", "ls -l /output/bin && ls -l /output"]