# syntax=docker/dockerfile:1
#
# Courtpit server image: a release build of `courtpit-server`, the exported web app it serves,
# and the Postgres client tools the nightly backup job shells out to (`pg_dump`). Five stages:
# chef (toolchain + cargo-chef), planner (dependency recipe), builder (cached dependency build,
# then the workspace), web (the Expo web export), runtime.
#
# There is deliberately no ENTRYPOINT: `CMD` runs the server, while Fly's
# `release_command = "courtpit-server migrate"` and the scheduled `courtpit-server tick`
# Machine replace the command and keep working (docs/deploy.md).

# Any Rust >= the workspace `rust-version` (1.88, edition 2024) works; CI builds with stable.
ARG RUST_VERSION=1.99
ARG DEBIAN_RELEASE=trixie
# pg_dump must be at least as new as the server it dumps. Neon creates PG 18 projects by
# default (and supports 14-18); a newer client dumps every older server, so track the newest
# major and bump it when Neon's default moves. Debian trixie itself only ships 17, hence PGDG.
ARG PG_MAJOR=18
# Node for the web build stage; the app's toolchain (Expo SDK 57) wants 22 or newer.
ARG NODE_VERSION=22

FROM rust:${RUST_VERSION}-slim-${DEBIAN_RELEASE} AS chef
# Smaller binary without touching Cargo.toml. Set here so `cook` and `build` agree on the profile.
ENV CARGO_PROFILE_RELEASE_STRIP=symbols
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Dependencies only: this layer is reused until Cargo.toml/Cargo.lock change.
RUN cargo chef cook --release --locked --package courtpit-server --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# `sqlx::migrate!("../../migrations")` embeds the SQL files at compile time.
COPY migrations migrations
RUN cargo build --release --locked --package courtpit-server --bin courtpit-server

# The web app: apps/mobile exported for browsers, served by `courtpit-server serve` from
# COURTPIT_WEB_DIR. One export serves every community host: it is built without
# EXPO_PUBLIC_COMMUNITY, so the browser's Host picks the community. Text files get a `.gz`
# twin that the server sends to clients accepting gzip.
FROM node:${NODE_VERSION}-slim AS web
WORKDIR /web
# Manifests first, so `npm ci` is reused until a manifest or the lockfile changes.
COPY package.json package-lock.json ./
COPY apps/mobile/package.json apps/mobile/
COPY packages/api-client/package.json packages/api-client/
RUN npm ci --no-audit --no-fund
COPY packages/api-client packages/api-client
COPY apps/mobile apps/mobile
RUN cd apps/mobile \
    && npx expo export --platform web --output-dir /web/dist \
    && find /web/dist -type f \( -name '*.html' -o -name '*.js' -o -name '*.css' -o -name '*.json' \
        -o -name '*.svg' -o -name '*.ttf' -o -name '*.map' \) -exec gzip -9 --keep {} +

FROM debian:${DEBIAN_RELEASE}-slim AS runtime
ARG DEBIAN_RELEASE
ARG PG_MAJOR
# postgresql-common ships the PGDG archive key; the repo is added by hand so the build needs
# neither curl nor gnupg.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates postgresql-common \
    && printf 'Types: deb\nURIs: https://apt.postgresql.org/pub/repos/apt\nSuites: %s-pgdg\nComponents: main\nSigned-By: /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc\n' \
        "${DEBIAN_RELEASE}" > /etc/apt/sources.list.d/pgdg.sources \
    && apt-get update \
    && apt-get install -y --no-install-recommends "postgresql-client-${PG_MAJOR}" \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --user-group --no-create-home --shell /usr/sbin/nologin courtpit
COPY --from=builder /app/target/release/courtpit-server /usr/local/bin/courtpit-server
COPY --from=web /web/dist /app/web
ENV COURTPIT_WEB_DIR=/app/web
USER courtpit
EXPOSE 8080
CMD ["courtpit-server", "serve"]
