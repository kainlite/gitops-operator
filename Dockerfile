FROM rust:alpine AS builder

WORKDIR /volume

# Install build dependencies for alpine/musl
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig git perl make

# Set OpenSSL to use static linking
ENV OPENSSL_STATIC=1
ENV OPENSSL_LIB_DIR=/usr/lib
ENV OPENSSL_INCLUDE_DIR=/usr/include

COPY Cargo.* .
COPY src src

RUN --mount=type=cache,target=/volume/target \
    --mount=type=cache,target=/root/.cargo/registry \
    cargo build --release --bin gitops-operator \
    && cp /volume/target/release/gitops-operator .

FROM cgr.dev/chainguard/static

COPY --from=builder --chown=nonroot:nonroot /volume/gitops-operator /app/
COPY files/known_hosts /home/nonroot/.ssh/known_hosts

EXPOSE 8000

ENTRYPOINT ["/app/gitops-operator"]
