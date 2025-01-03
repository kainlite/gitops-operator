FROM --platform=$BUILDPLATFORM clux/muslrust:stable AS builder

ARG TARGETPLATFORM

WORKDIR /app
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    case "$TARGETPLATFORM" in \
        "linux/amd64")  RUST_TARGET="x86_64-unknown-linux-musl" ;; \
        "linux/arm64")  RUST_TARGET="aarch64-unknown-linux-musl" ;; \
        "linux/arm/v7") RUST_TARGET="armv7-unknown-linux-musleabihf" ;; \
        *) exit 1 ;; \
    esac && \
    rustup target add "$RUST_TARGET" && \
    cargo fetch

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target-${TARGETPLATFORM//\//-} \
    case "$TARGETPLATFORM" in \
        "linux/amd64")  RUST_TARGET="x86_64-unknown-linux-musl" ;; \
        "linux/arm64")  RUST_TARGET="aarch64-unknown-linux-musl" ;; \
        "linux/arm/v7") RUST_TARGET="armv7-unknown-linux-musleabihf" ;; \
        *) exit 1 ;; \
    esac && \
    RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --target "$RUST_TARGET" && \
    strip "target/$RUST_TARGET/release/gitops-operator" && \
    mv "target/$RUST_TARGET/release/gitops-operator" /gitops-operator

FROM cgr.dev/chainguard/static:latest

COPY --from=builder --chown=nonroot:nonroot /gitops-operator /app/
COPY --from=builder --chown=nonroot:nonroot /app/files/known_hosts /home/nonroot/.ssh/known_hosts

EXPOSE 8000
ENTRYPOINT ["/app/gitops-operator"]
