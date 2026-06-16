# gitops-operator

[![ci](https://github.com/kainlite/gitops-operator/actions/workflows/ci.yaml/badge.svg)](https://github.com/kainlite/gitops-operator/actions/workflows/ci.yaml)
[![docker image](https://ghcr.io/kainlite/gitops-operator)](https://github.com/kainlite/gitops-operator/pkgs/container/gitops-operator)
[![codecov](https://codecov.io/gh/kainlite/gitops-operator/branch/master/graph/badge.svg)](https://codecov.io/gh/kainlite/gitops-operator)

Basically this is a GitOps controller working in pull mode (from the cluster), triggered by Kubernetes on a readiness check (this is a
hack) to trigger our reconcile method, while this is a learning project maybe some day it will be able to handle serious
workloads.

I personally use it in my cluster to manage the lifecycle of my blog and the operator itself in k3s (my production
cluster), before this operator existed I used Argo CD image updater, I still use argo but git dictates which image is
running.

## How it works

The operator runs a Kubernetes reflector that watches all `Deployment` objects in the cluster (read-only). A reconcile
pass is triggered by an HTTP `GET /reconcile` (wired to a readiness check, hence the "hack"). For every deployment that
carries the required `gitops.operator.*` annotations and has `enabled: true`, it:

1. Loads the SSH key and any optional registry, notification, and GitHub-token secrets.
2. Clones (or fast-forward updates) both the **app** repository and the **manifests** repository on the configured
   `observe_branch` (default `master`).
3. Reads the latest commit SHA from the app repository (full 40-char or 7-char, per `tag_type`).
4. Compares it against the image tag in the manifest's `deployment_path`. If they already match, the deployment is
   reported as `up_to_date` and left untouched.
5. Otherwise, optionally waits for the image to appear in the registry, using GitHub Actions build status (when a token
   is configured) to retry with exponential backoff while the build is still running.
6. Patches the image tag in the manifest, commits, and pushes back to the manifests repository on `observe_branch`.
7. Optionally sends Slack-formatted notifications along the way.

Your CD tool (Argo CD in my case) then rolls out the new image because the manifests repository changed. The operator
never deploys directly; git remains the source of truth. Each deployment's outcome is returned in the structured
[`/reconcile` response](#api).

## Article
You can [read](https://segfault.pw/en/blog/create-your-own-gitops-controller-with-rust) or watch the video here (coming soon tm)... 

### Locally
Run against your current Kubernetes context:

```sh
kind create cluster
## Apply the manifests from the gitops-operator-manifests to manage that repo (otherwise deploy your own app with the
## annotations)
# kustomize build . | kubectl apply -f -

cargo watch -- cargo run
# or handy to debug and be able to read logs and events from the tracer
RUST_LOG=info cargo watch -- cargo run | jq -R '. as $line | try (fromjson | .time + " " + .msg + " " + .target) catch $line'
# or using bunyan
RUST_LOG=info cargo watch -- cargo run | bunyan
# or from the deployed version
stern -o raw -n gitops-operator gitops | jq -R '. as $line | try (fromjson | .time + " " + .msg + " " + .target) catch $line'
# or using bunyan
stern -o raw -n gitops-operator gitops | bunyan
```

### Observability stack (tempo, prometheus, and grafana)
To run the observability stack run (note that these run on the host's ports due to we need to connect to the cluster
using the local configuration):
```
docker compose up -d
```
Traces are collected by Tempo (OTLP on port 4317/4318), queryable via Grafana at `http://localhost:3000`.
Prometheus is available at `http://localhost:9090` and Tempo API at `http://localhost:3200`.

### Running the application
To observe a deployment just add these annotations to your configuration file (this is what I'm using to self-observe
and update the manifests repo for this project). The operator only processes a deployment when **all required
annotations** are present and `gitops.operator.enabled` is `true`; otherwise the deployment is skipped.

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  annotations:
    # --- required ---
    gitops.operator.enabled: 'true'
    gitops.operator.app_repository: git@github.com:kainlite/gitops-operator.git
    gitops.operator.manifest_repository: git@github.com:kainlite/gitops-operator-manifests.git
    gitops.operator.deployment_path: app/00-deployment.yaml
    gitops.operator.image_name: kainlite/gitops-operator
    gitops.operator.ssh_key_name: ssh-key
    gitops.operator.ssh_key_namespace: gitops-operator
    # --- optional ---
    gitops.operator.notifications_secret_name: 'webhook-secret'
    gitops.operator.notifications_secret_namespace: 'gitops-operator'
    gitops.operator.registry_secret_name: 'regcred'
    gitops.operator.registry_secret_namespace: 'gitops-operator'
  labels:
    app: gitops-operator
  name: gitops-operator
  namespace: default
spec:
  replicas: 1
...
```

> Note: the operator reads the deployment's own `metadata.namespace`; there is no `gitops.operator.namespace`
> annotation.

**Required annotations** (missing any one causes the deployment to be skipped):

    gitops.operator.enabled              # Whether the operator should process this deployment ('true' to enable)
    gitops.operator.app_repository       # Application repository, SSH format (git@host:owner/repo.git)
    gitops.operator.manifest_repository  # Manifests repository, SSH format (git@host:owner/repo.git)
    gitops.operator.deployment_path      # Path to the deployment file inside the manifests repository
    gitops.operator.image_name           # Image name the operator looks for and patches (e.g. kainlite/gitops-operator)
    gitops.operator.ssh_key_name         # Name of the secret containing the SSH key
    gitops.operator.ssh_key_namespace    # Namespace of the secret containing the SSH key

**Optional annotations**:

    gitops.operator.observe_branch                  # Branch to track in both repositories (default: master)
    gitops.operator.tag_type                        # Image tag form: 'long' (40-char SHA) or 'short' (7-char SHA) (default: long)
    gitops.operator.notifications_secret_name       # Secret holding a Slack-compatible webhook URL (key: webhook-url)
    gitops.operator.notifications_secret_namespace  # Namespace of the notifications secret (default: gitops-operator)
    gitops.operator.registry_secret_url             # Registry URL for image existence checks (default: https://index.docker.io/v1/)
    gitops.operator.registry_secret_name            # Name of the docker-registry secret (default: regcred)
    gitops.operator.registry_secret_namespace       # Namespace of the registry secret (default: gitops-operator)
    gitops.operator.github_token_secret_name        # Secret holding a GitHub token (key: github-token) for build status checks
    gitops.operator.github_token_secret_namespace   # Namespace of the GitHub token secret (default: gitops-operator)

### SSH key secret
Note: you can create the secret as follows:
```
kubectl -n gitops-operator create secret generic ssh-key --from-file=ssh-privatekey=/home/user/.ssh/id_rsa
```
If you don't want the operator to be able to read all secrets you can limit it with RBAC, it will attempt to read only what you tell it to anyway.

You might be wondering why do you need an SSH key? short answer to fetch and write to your repository, why SSH? well it
is a secure authentication mechanism and it is widely adopted making the operator provider independent, it doesn't
matter which hosting solution you prefer it should still work the very same way as long as it supports SSH
authentication.

### Notifications
In order to be able to send notifications (following the Slack format), you can create a secret like that (You will need
to create a secret per namespace, where you app is deployed):
```
kubectl create secret generic webhook-secret  -n define_ns --from-literal=webhook-url=https://hooks.slack.com/services/...
```

### Enable checking the container registry
In order to check if the image is already present in the repository before patching the files you'll need a secret for
the container registry which can be created like this (these annotations are optional by default):

For Docker Hub:
```bash
docker login
kubectl -n gitops-operator create secret docker-registry regcred --from-file=/home/user/.docker/config.json
```

For GHCR (GitHub Container Registry):
```bash
echo $GITHUB_TOKEN | docker login ghcr.io -u USERNAME --password-stdin
kubectl -n gitops-operator create secret docker-registry regcred --from-file=/home/user/.docker/config.json
```
Then set `gitops.operator.registry_secret_url: 'https://ghcr.io'` in your deployment annotations.

### Enable GitHub Actions build status checks
When an image is not found in the registry, the operator can check GitHub Actions to determine if a build is still
running and retry with exponential backoff. This is optional and requires a GitHub token:
```bash
kubectl -n gitops-operator create secret generic github-token --from-literal=github-token=ghp_your_token_here
```
Then set `gitops.operator.github_token_secret_name: 'github-token'` in your deployment annotations.
The token needs `actions:read` permission on the repository.

### In-Cluster
Apply manifests from [here](https://github.com/kainlite/gitops-operator-manifests), then you can trigger it manually using port-forward: `kubectl port-forward service/gitops-operator 8000:80`

### Api
The operator exposes the following HTTP endpoints on port `8000`:

| Endpoint     | Description                                                                  |
| ------------ | ---------------------------------------------------------------------------- |
| `/reconcile` | Triggers a reconcile pass and returns a structured result per deployment     |
| `/status`    | Human-readable table of the deployments the operator currently tracks        |
| `/debug`     | Full parsed configuration for every tracked deployment (JSON)                |
| `/health`    | Liveness/readiness probe; also reports how many deployments are tracked      |
| `/metrics`   | Prometheus metrics                                                           |

You can trigger the reconcile method from the following URL (explanation in the post/video, this is a hack, not a real
reconcile method however it does the trick for this case). Each entry identifies the deployment, what action was taken,
and the SHA transition:

```sh
$ curl 0.0.0.0:8000/reconcile | jq
[
  {
    "deployment": "gitops-operator",
    "namespace": "gitops-operator",
    "action": "patched",
    "from_sha": "3c0a88249fb61a0a4f4a65295f42b2dee3963c28",
    "to_sha": "e4f5a6b1c2d3e4f5a6b1c2d3e4f5a6b1c2d3e4f5",
    "status": "success",
    "message": "Deployment gitops-operator patched successfully to version e4f5a6b1c2d3e4f5a6b1c2d3e4f5a6b1c2d3e4f5"
  }
]
```

`action` is one of `patched`, `up_to_date`, `skipped` (disabled deployments) or `failed`; `status` is `success`,
`failure` or `skipped`. `from_sha`/`to_sha` are omitted when not applicable.

Status endpoint (human-readable):
```sh
$ curl 0.0.0.0:8000/status
gitops-operator status
tracked deployments: 2

NAMESPACE    DEPLOYMENT               ENABLED  BRANCH   IMAGE
default      blog                     true     master   kainlite/blog:3c0a88249fb61a0a...
default      api                      false    main     kainlite/api:9f2b1c4
```

Health endpoint:
```sh
$ curl 0.0.0.0:8000/health
{"status":"ok","tracked_deployments":1}
```

Debug endpoint:
```sh
❯ curl localhost:8000/debug | jq
[
  {
    "container": "kainlite/gitops-operator",
    "name": "gitops-operator",
    "namespace": "gitops-operator",
    "annotations": {
      "deployment.kubernetes.io/revision": "3",
      "gitops.operator.app_repository": "git@github.com:kainlite/gitops-operator.git",
      "gitops.operator.deployment_path": "app/00-deployment.yaml",
      "gitops.operator.enabled": "true",
      "gitops.operator.image_name": "kainlite/gitops-operator",
      "gitops.operator.manifest_repository": "git@github.com:kainlite/gitops-operator-manifests.git",
      "gitops.operator.ssh_key_name": "ssh-key",
      "gitops.operator.ssh_key_namespace": "gitops-operator"
    },
    "version": "3c0a88249fb61a0a4f4a65295f42b2dee3963c28",
    "config": {
      "enabled": true,
      "namespace": "gitops-operator",
      "app_repository": "git@github.com:kainlite/gitops-operator.git",
      "manifest_repository": "git@github.com:kainlite/gitops-operator-manifests.git",
      "image_name": "kainlite/gitops-operator",
      "deployment_path": "app/00-deployment.yaml",
      "observe_branch": "master",
      "tag_type": "long",
      "ssh_key_name": "ssh-key",
      "ssh_key_namespace": "gitops-operator",
      "notifications_secret_name": null,
      "notifications_secret_namespace": null,
      "registry_url": null,
      "registry_secret_name": null,
      "registry_secret_namespace": null,
      "github_token_secret_name": null,
      "github_token_secret_namespace": null
    }
  }
]
```

### Developing
- Locally against a cluster: `cargo watch`
- In-cluster: edit and `tilt up` [*](https://tilt.dev/)
- Docker build & import to kind: `just build && just import`

### Notes
This was created based in the example [here](https://github.com/kube-rs/version-rs) from [kube-rs](https://github.com/kube-rs)
