FROM rust:1.83 AS builder

COPY Cargo.* .
COPY *.rs .

RUN apt update && apt install -y libgit2-dev && cargo build --release --bin gitops-operator

FROM rust:1.83-slim

COPY --from=builder --chown=nonroot:nonroot /target/release/gitops-operator /app/
COPY files/known_hosts /home/nonroot/.ssh/known_hosts

EXPOSE 8000

ENTRYPOINT ["/app/gitops-operator"]
