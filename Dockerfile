# City game — single image, two modes.
#
# The browser talks to the backend directly (same-origin /api + /ws), so the
# production topology is the DEFAULT mode: city-server serving the static web
# build, the HTTP API, and the WebSocket on one port. MODE=web runs busybox
# httpd serving only the static build (standalone static hosting).
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
RUN addgroup -g 65532 -S app \
 && adduser -u 65532 -S -G app app \
 && mkdir -p /data /app \
 && ln -s /data /app/data \
 && chown -R app:app /data /app
COPY --from=builder /out/bin/city-server /usr/local/bin/city-server
COPY --from=web-build /src/dist /app/apps/web/dist

ENV CITY_BIND=0.0.0.0:8080
EXPOSE 8080
USER 65532:65532
WORKDIR /app

# MODE=web: serve only the static build; default: full game server.
CMD ["sh", "-c", "if [ \"$MODE\" = \"web\" ]; then exec busybox httpd -f -p 0.0.0.0:8080 -h /app/apps/web/dist; else exec city-server; fi"]
