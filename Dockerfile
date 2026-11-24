# syntax=docker/dockerfile:1

FROM rust:1.100.0-slim-trixie AS build
WORKDIR /app

# Give Cargo a binary target without depending on the real source files.
RUN --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    mkdir src && touch src/main.rs && cargo fetch --locked

RUN --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=bind,source=src,target=src \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked

FROM gcr.io/distroless/cc-debian13:nonroot AS runtime
LABEL org.opencontainers.image.source="https://github.com/jairo-bosch/ado-code-review"
COPY --link --from=build /app/target/release/ado-code-review /usr/local/bin/ado-code-review
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/ado-code-review"]
