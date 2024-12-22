FROM clux/muslrust:stable AS builder

COPY Cargo.* .
COPY *.rs .

RUN --mount=type=cache,target=/volume/target \
    --mount=type=cache,target=/root/.cargo/registry \
    cargo build --release --bin gitops-operator && \
    mv /volume/target/x86_64-unknown-linux-musl/release/gitops-operator .

FROM cgr.dev/chainguard/static

COPY --from=builder --chown=nonroot:nonroot /volume/gitops-operator /app/
COPY files/known_hosts /home/nonroot/.ssh/known_hosts

EXPOSE 8080

ENTRYPOINT ["/app/gitops-operator"]
