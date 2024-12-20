[private]
default:
  @just --list --unsorted

run:
  RUST_LOG=debug,hyper=info,rustls=info cargo run

fmt:
  cargo +nightly fmt

build:
  docker build -t kainlite/gitops-operator:local .

[private]
release:
  cargo release patch --execute

[private]
import:
  k3d image import kainlite/gitops-operator:local --cluster main
  sd "image: .*" "image: kainlite/gitops-operator:local" deployment.yaml
  kubectl apply -f deployment.yaml
