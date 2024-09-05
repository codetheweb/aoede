FROM rust:alpine AS dependencies

RUN apk add --no-cache alpine-sdk cmake automake autoconf opus libtool
RUN cargo install cargo-chef

FROM dependencies AS planner
WORKDIR app

# We only pay the installation cost once,
# it will be cached from the second build onwards
# To ensure a reproducible build consider pinning
# the cargo-chef version with `--version X.X.X`
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM dependencies AS cacher
WORKDIR app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM dependencies AS builder
WORKDIR app
COPY . .
# Copy over the cached dependencies
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo build --release --bin aoede

FROM alpine AS runtime
WORKDIR app
COPY --from=builder /app/target/release/aoede /usr/local/bin

ENV CACHE_DIR=/data

ENTRYPOINT ["/usr/local/bin/aoede"]
