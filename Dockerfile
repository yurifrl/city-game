# City game — one image, one process.
#
# city-server owns the world, renders every seat's view with wgpu, encodes
# frames and serves the Svelte viewer, /api, /ws (game wire) and /stream
# (video) on one port. The browser only plays video and sends raw input.
#
# Rendering needs a Vulkan device. This image ships Mesa's software Vulkan
# (lavapipe) so it runs on CPU-only nodes; a node with a GPU driver mounted in
# would be picked instead. If no adapter exists city-server exits at boot.
#
# ---- Viewer build -------------------------------------------------------------
FROM oven/bun:1-alpine AS viewer-build
WORKDIR /src
COPY apps/viewer/package.json apps/viewer/bun.lock ./
RUN bun install --frozen-lockfile
COPY apps/viewer/ ./
RUN bun run build

# ---- Rust server build --------------------------------------------------------
# cargo-chef splits the dependency compile into its own layer so the long
# build is cached across source-only changes (and carried by the GHA cache
# into CI, where every build starts with a cold Docker cache).
FROM rust:1.99-alpine3.22 AS chef
RUN apk add --no-cache musl-dev && cargo install cargo-chef
# wgpu loads libvulkan at runtime with dlopen, which a statically linked musl
# binary cannot do. Link dynamically against musl instead.
ENV RUSTFLAGS="-C target-feature=-crt-static"

FROM chef AS planner
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
WORKDIR /build
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo install --locked --path crates/server --root /out

# ---- Runtime ------------------------------------------------------------------
# city-server resolves DIST_DIR and DB_PATH relative to its working directory
# (./apps/viewer/dist, ./data/city.db — see crates/server/src/main.rs), so the
# runtime recreates that layout: dist under /app/apps/viewer/dist, SQLite state
# on the mounted volume via /app/data -> /data.
FROM alpine:3.22 AS runtime
# vulkan-loader provides libvulkan.so.1; mesa-vulkan-swrast is lavapipe, the
# CPU Vulkan driver (amd64 and arm64); libgcc is needed by the dynamic binary.
# The install must not be optional: without a Vulkan device the server cannot
# start, so a missing package is a build failure, not a fallback.
RUN addgroup -g 65532 -S app \
 && adduser -u 65532 -S -G app app \
 && mkdir -p /data /app \
 && ln -s /data /app/data \
 && chown -R app:app /data /app \
 && apk add --no-cache libgcc vulkan-loader mesa-vulkan-swrast
COPY --from=builder /out/bin/city-server /usr/local/bin/city-server
COPY --from=viewer-build /src/dist /app/apps/viewer/dist

ENV CITY_BIND=0.0.0.0:8080
EXPOSE 8080
USER 65532:65532
WORKDIR /app

CMD ["city-server"]
