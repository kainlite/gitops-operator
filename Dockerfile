ARG BUILDPLATFORM
FROM --platform=$BUILDPLATFORM rust:1.83 AS builder

COPY Cargo.* .
COPY *.rs .

RUN --mount=type=cache,target=/volume/target \
    --mount=type=cache,target=/root/.cargo/registry \
    cargo build --release --bin gitops-operator && \
    if [ "$TARGETPLATFORM" = "linux/amd64" ]; then ARCHITECTURE=x86_64; elif [ "$TARGETPLATFORM" = "linux/arm/v7" ]; then ARCHITECTURE=arm32v7; elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then ARCHITECTURE=aarch64; else ARCHITECTURE=x86_64; fi && \
    mv /volume/target/$ARCHITECTURE-unknown-linux-musl/release/gitops-operator .

# FROM cgr.dev/chainguard/static
FROM --platform=$BUILDPLATFORM rust:1.83-slim

COPY --from=builder --chown=nonroot:nonroot /volume/gitops-operator /app/
COPY files/known_hosts /home/nonroot/.ssh/known_hosts

EXPOSE 8000

ENTRYPOINT ["/app/gitops-operator"]
