# version-rs
[![ci](https://github.com/kainlite/gitops-operator/actions/workflows/ci.yml/badge.svg)](https://github.com/kainlite/gitops-operator/actions/workflows/ci.yml)
[![docker image](https://img.shields.io/docker/pulls/kainlite/gitops-operator.svg)](
https://hub.docker.com/r/kainlite/gitops-operator/tags/)

This was created based in the example [here](https://github.com/kube-rs/version-rs) from [kube-rs](https://github.com/kube-rs)

Basically this is a GitOps controller working in pull mode, triggered by Kubernetes on a readiness check (this is a
hack) to trigger our fake reconcile method, this can be considered a toy controller or learning project.

## Article
You can read or watch the video here... (coming soon tm)...

### Locally
Run against your current Kubernetes context:

```sh
cargo run
```

### In-Cluster
Apply manifests from [here](https://github.com/kainlite/gitops-operator-manifests), then `kubectl port-forward service/gitops-operator 8000:80`

### Api
You can trigger the reconcile method from the following URL (explanation in the post/video, this is a hack, not a real
reconcile method):

```sh
$ curl 0.0.0.0:8000/reconcile
[{"container":"clux/controller","name":"foo-controller","version":"latest"},{"container":"alpine","name":"debugger","version":"3.13"}]
```

## Developing
- Locally against a cluster: `cargo run`
- In-cluster: edit and `tilt up` [*](https://tilt.dev/)
- Docker build: `just build`
