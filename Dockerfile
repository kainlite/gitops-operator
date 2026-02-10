FROM --platform=$BUILDPLATFORM clux/muslrust:stable AS builder

ARG TARGETPLATFORM

WORKDIR /volume

# Install cross-compilation tools for musl
# Try multiple mirrors since musl.cc can be unreliable
RUN apt-get update && apt-get install -y musl-tools wget ca-certificates \
    && (wget --timeout=30 -q https://more.musl.cc/11/x86_64-linux-musl/aarch64-linux-musl-cross.tgz \
        -O /tmp/aarch64-linux-musl-cross.tgz \
        || wget --timeout=30 -q https://musl.cc/aarch64-linux-musl-cross.tgz \
        -O /tmp/aarch64-linux-musl-cross.tgz) \
    && tar -xf /tmp/aarch64-linux-musl-cross.tgz -C /usr \
    && rm /tmp/aarch64-linux-musl-cross.tgz

# Install cross-compilation target
RUN rustup target add aarch64-unknown-linux-musl x86_64-unknown-linux-musl

COPY Cargo.* .
COPY src src

RUN --mount=type=cache,target=/volume/target \
    --mount=type=cache,target=/root/.cargo/registry \
    if [ "$TARGETPLATFORM" = "linux/amd64" ]; then \
        ARCHITECTURE=x86_64; \
        cargo build --release --bin gitops-operator --target x86_64-unknown-linux-musl; \
    elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
        ARCHITECTURE=aarch64; \
        PATH="/usr/aarch64-linux-musl-cross/bin:$PATH" \
        CC_aarch64_unknown_linux_musl=aarch64-linux-musl-gcc \
        AR_aarch64_unknown_linux_musl=aarch64-linux-musl-ar \
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc \
        cargo build --release --bin gitops-operator --target aarch64-unknown-linux-musl; \
    else \
        ARCHITECTURE=x86_64; \
        cargo build --release --bin gitops-operator --target x86_64-unknown-linux-musl; \
    fi && \
    mv /volume/target/$ARCHITECTURE-unknown-linux-musl/release/gitops-operator .

FROM cgr.dev/chainguard/static

COPY --from=builder --chown=nonroot:nonroot /volume/gitops-operator /app/
COPY files/known_hosts /home/nonroot/.ssh/known_hosts

EXPOSE 8000

ENTRYPOINT ["/app/gitops-operator"]
