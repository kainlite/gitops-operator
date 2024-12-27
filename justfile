[private]
default:
  @just --list --unsorted

run:
  RUST_LOG=debug,hyper=info,rustls=info cargo run

fmt:
  cargo +nightly fmt

build:
  docker build -t kainlite/gitops-operator:local .
  docker push kainlite/gitops-operator:local

[private]
release:
  cargo release patch --execute

[private]
import:
  kind load docker-image kainlite/gitops-operator:local
  kubectl patch deployment gitops-operator -p '{"spec":{"template":{"spec":{"containers":[{"name":"gitops-operator","image":"kainlite/gitops-operator:local"}]}}}}'
  kubectl rollout restart deploy gitops-operator
