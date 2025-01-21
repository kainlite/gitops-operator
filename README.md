# gitops-operator

[![ci](https://github.com/kainlite/gitops-operator/actions/workflows/ci.yml/badge.svg)](https://github.com/kainlite/gitops-operator/actions/workflows/ci.yml)
[![docker image](https://img.shields.io/docker/pulls/kainlite/gitops-operator.svg)](https://hub.docker.com/r/kainlite/gitops-operator)
[![codecov](https://codecov.io/gh/kainlite/gitops-operator/branch/master/graph/badge.svg)](https://codecov.io/gh/kainlite/gitops-operator)

https://hub.docker.com/r/kainlite/gitops-operator/tags/)

Basically this is a GitOps controller working in pull mode (from the cluster), triggered by Kubernetes on a readiness check (this is a
hack) to trigger our reconcile method, while this is a learning project maybe some day it will be able to handle serious
workloads.

I personally use it in my cluster to manage the lifecycle of my blog and the operator itself in k3s (my production
cluster), before this operator existed I used Argo CD image updater, I still use argo but git dictates which image is
running.

## Article
You can [read](https://redbeard.team/en/blog/create-your-own-gitops-controller-with-rust) or watch the video here (coming soon tm)... 

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

### Observability stack (jaegger and prometheus)
To run the observability stack run (note that these run on the host's ports due to we need to connect to the cluster
using the local configuration):
```
docker compose up -d
```

### Running the application
To observe a deployment just add these annotations to your configuration file (this is what I'm using to self-observe
and update the manifests repo for this project):

These are all required fields, or the deployment will be skipped:
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  annotations:
    gitops.operator.app_repository: git@github.com:kainlite/gitops-operator.git
    gitops.operator.deployment_path: app/00-deployment.yaml
    gitops.operator.enabled: 'true'
    gitops.operator.image_name: kainlite/gitops-operator
    gitops.operator.manifest_repository: git@github.com:kainlite/gitops-operator-manifests.git
    gitops.operator.namespace: default
    gitops.operator.ssh_key_name: ssh-key
    gitops.operator.ssh_key_namespace: gitops-operator
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

A bit more information about the annotations:

    gitops.operator.enabled: # Wheter the operator should process this deployment or not
    gitops.operator.app_repository: # Git repository with SSH format for the application repository
    gitops.operator.manifest_repository: # Git repository with SSH format for the manifests repository
    gitops.operator.deployment_path: # Location of the deployment file in the manifests repository
    gitops.operator.image_name: # The complete image name that the operator should be looking for
    gitops.operator.namespace: # The namespace where this deployment is currently running
    gitops.operator.ssh_key_name: # The name of the secret containing the SSH key
    gitops.operator.ssh_key_namespace: # The namespace of the secret containing the SSH key
    gitops.operator.notifications_secret_name: # OPTIONAL: Wether to try to send a Slack notification to the provided endpoint via the secret (the data field needs to be webhook-url)
    gitops.operator.notifications_secret_namespace: # OPTIONAL: Wether to try to send a Slack notification to the provided endpoint via the secret (the data field needs to be webhook-url)
    gitops.operator.registry_secret_name: # OPTIONAL: defaults to regcred
    gitops.operator.registry_secret_namespace: # OPTIONAL: defaults to gitops-operator

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

### Enable checking the Docker registry
In order to check if the image is already present in the repository before patching the files you'll need a secret for
the docker registry which can be created like this (these annotations are optional by default):
```bash
docker login
kubectl -n gitops-operator create secret docker-registry regcred --from-file=/home/user/.docker/config.json
```

### In-Cluster
Apply manifests from [here](https://github.com/kainlite/gitops-operator-manifests), then you can trigger it manually using port-forward: `kubectl port-forward service/gitops-operator 8000:80`

### Api
You can trigger the reconcile method from the following URL (explanation in the post/video, this is a hack, not a real
reconcile method however it does the trick for this case):

```sh
$ curl 0.0.0.0:8000/reconcile
[
  {
    "Success": "Deployment: gitops-operator is up to date, proceeding to next deployment..."
  }
]
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
      "gitops.operator.namespace": "gitops-operator",
      "gitops.operator.notifications": "true",
      "gitops.operator.ssh_key_name": "ssh-key",
      "gitops.operator.ssh_key_namespace": "gitops-operator",
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
      "state": "Queued"
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
