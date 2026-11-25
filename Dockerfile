# syntax=docker/dockerfile:1

FROM rust:1.100.0-slim-trixie AS build
WORKDIR /app

# Give Cargo a binary target without depending on the real source files.
RUN --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=bind,source=xtask/Cargo.toml,target=xtask/Cargo.toml \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    mkdir -p src xtask/src && touch src/main.rs xtask/src/main.rs && cargo fetch --locked

RUN --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=xtask/Cargo.toml,target=xtask/Cargo.toml \
    --mount=type=bind,source=xtask/src,target=xtask/src \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked -p ado-code-review

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:532c7a94f9e96ba9db75eb9388f4227751131c7cb0a411d1a8521af4096135de AS runtime
LABEL org.opencontainers.image.source="https://github.com/jairo-bosch/ado-code-review"
COPY --link --from=build /app/target/release/ado-code-review /usr/local/bin/ado-code-review
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/ado-code-review"]
