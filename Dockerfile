# City game — single image, three modes.
#
# The browser talks to the backend directly (same-origin /api + /ws), so the
# direct-client topology is the DEFAULT mode: city-server serving the static
# web build, the HTTP API, and the WebSocket on one port. MODE=web runs
# busybox httpd serving only the static build (standalone static hosting).
# MODE=stream runs the video bridge (apps/stream): warm headless Chromium
# seats render the game server-side and pixel-only viewers watch/play through
# them. It expects to run beside a DEFAULT-mode container in the same pod and
# reaches it at CITY_GAME_URL (default http://127.0.0.1:8080).
#
# ---- Web client build ---------------------------------------------------------
FROM oven/bun:1-alpine AS web-build
WORKDIR /src
COPY apps/web/package.json apps/web/bun.lock ./
RUN bun install --frozen-lockfile
COPY apps/web/ ./
RUN bun run build

# ---- Rust server build --------------------------------------------------------
# cargo-chef splits the dependency compile into its own layer so the long
# build is cached across source-only changes (and carried by the GHA cache
# into CI, where every build starts with a cold Docker cache).
FROM rust:1.99-alpine3.22 AS chef
RUN apk add --no-cache musl-dev && cargo install cargo-chef

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
# (./apps/web/dist, ./data/city.db — see crates/server/src/main.rs), so the
# runtime recreates that layout: dist under /app/apps/web/dist, SQLite state
# on the mounted volume via /app/data -> /data.
FROM alpine:3.22 AS runtime
# Chromium + Noto fonts serve MODE=stream seats; bun runs the stream bridge.
# chromium-swiftshader ships the software Vulkan ICD (SwANGLE — ANGLE on
# SwiftShader) that WebGL/WebGPU render through on CPU-only nodes; it exists
# for both amd64 and arm64. The install must not be optional: without it the
# seat renders a dead "backend: unavailable" screen, so a missing package is a
# build failure, not a fallback.
# The ICD lands in /usr/lib/chromium/ (not the loader's default search path),
# so VK_ICD_FILENAMES points the loader at it explicitly.
# Non-essential locale/l10n deps are skipped explicitly to keep the layer small.
RUN addgroup -g 65532 -S app \
 && adduser -u 65532 -S -G app app \
 && mkdir -p /data /app \
 && ln -s /data /app/data \
 && chown -R app:app /data /app \
 && apk add --no-cache chromium chromium-swiftshader font-noto
COPY --from=builder /out/bin/city-server /usr/local/bin/city-server
COPY --from=web-build /src/dist /app/apps/web/dist
COPY --from=oven/bun:1-alpine /usr/local/bin/bun /usr/local/bin/bun
# The bridge has no runtime dependencies (Bun standard library only); sources
# run directly under bun with no install step.
COPY apps/stream/ /app/apps/stream/

ENV CITY_BIND=0.0.0.0:8080
ENV CITY_STREAM_CHROME=/usr/bin/chromium-browser
ENV VK_ICD_FILENAMES=/usr/lib/chromium/vk_swiftshader_icd.json
EXPOSE 8080
EXPOSE 8092
USER 65532:65532
WORKDIR /app

# MODE=web: serve only the static build; MODE=stream: the video bridge;
# default: full game server.
CMD ["sh", "-c", "if [ \"$MODE\" = \"web\" ]; then exec busybox httpd -f -p 0.0.0.0:8080 -h /app/apps/web/dist; elif [ \"$MODE\" = \"stream\" ]; then exec bun /app/apps/stream/src/index.ts; else exec city-server; fi"]
