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
    gitops.operator.notifications: true
  labels:
    app: gitops-operator
  name: gitops-operator
  namespace: default
spec:
  replicas: 1
...
```

A bit more information about the annotations:
```
    gitops.operator.enabled: # Wheter the operator should process this deployment or not
    gitops.operator.app_repository: # Git repository with SSH format for the application repository
    gitops.operator.manifest_repository: # Git repository with SSH format for the manifests repository
    gitops.operator.deployment_path: # Location of the deployment file in the manifests repository
    gitops.operator.image_name: # The complete image name that the operator should be looking for
    gitops.operator.namespace: # The namespace where this deployment is currently running
    gitops.operator.ssh_key_name: # The name of the secret containing the SSH key
    gitops.operator.ssh_key_namespace: # The namespace of the secret containing the SSH key
    gitops.operator.notifications: # Wether to try to send a Slack notification to the provided endpoint via the secret
```

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
In order to be able to send notifications (following the Slack format), you can create a secret like that (this is a
global config):
```
kubectl create secret generic webhook-secret  -n gitops-operator --from-literal=webhook-url=https://hooks.slack.com/services/...
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
    "container": "kainlite/gitops-operator",
    "name": "gitops-operator",
    "namespace": "default",
    "annotations": {
      "deployment.kubernetes.io/revision": "34",
      "gitops.operator.app_repository": "git@github.com:kainlite/gitops-operator.git",
      "gitops.operator.deployment_path": "app/00-deployment.yaml",
      "gitops.operator.enabled": "true",
      "gitops.operator.image_name": "kainlite/gitops-operator",
      "gitops.operator.manifest_repository": "git@github.com:kainlite/gitops-operator-manifests.git",
      "gitops.operator.namespace": "default",
      "kubectl.kubernetes.io/last-applied-configuration": "{\"apiVersion\":\"apps/v1\",\"kind\":\"Deployment\",\"metadata\":{\"annotations\":{\"gitops.operator.app_repository\":\"git@github.com:kainlite/gitops-operator.git\",\"gitops.operator.deployment_path\":\"app/00-deployment.yaml\",\"gitops.operator.enabled\":\"true\",\"gitops.operator.image_name\":\"kainlite/gitops-operator\",\"gitops.operator.manifest_repository\":\"git@github.com:kainlite/gitops-operator-manifests.git\",\"gitops.operator.namespace\":\"default\"},\"labels\":{\"app\":\"gitops-operator\",\"argocd.argoproj.io/instance\":\"gitops-operator\"},\"name\":\"gitops-operator\",\"namespace\":\"default\"},\"spec\":{\"replicas\":1,\"selector\":{\"matchLabels\":{\"app\":\"gitops-operator\"}},\"template\":{\"metadata\":{\"labels\":{\"app\":\"gitops-operator\"}},\"spec\":{\"containers\":[{\"image\":\"kainlite/gitops-operator:a57e6e3a195464a8bbbdc1bff3a6f70ed236154d\",\"imagePullPolicy\":\"Always\",\"livenessProbe\":{\"failureThreshold\":5,\"httpGet\":{\"path\":\"/health\",\"port\":\"http\"},\"periodSeconds\":15},\"name\":\"gitops-operator\",\"ports\":[{\"containerPort\":8000,\"name\":\"http\",\"protocol\":\"TCP\"}],\"readinessProbe\":{\"httpGet\":{\"path\":\"/reconcile\",\"port\":\"http\"},\"initialDelaySeconds\":60,\"periodSeconds\":120,\"timeoutSeconds\":60},\"resources\":{\"limits\":{\"cpu\":\"1000m\",\"memory\":\"1024Mi\"},\"requests\":{\"cpu\":\"500m\",\"memory\":\"100Mi\"}},\"volumeMounts\":[{\"mountPath\":\"/home/nonroot/.ssh/id_rsa_demo\",\"name\":\"my-ssh-key\",\"readOnly\":true,\"subPath\":\"ssh-privatekey\"}]}],\"serviceAccountName\":\"gitops-operator\",\"volumes\":[{\"name\":\"my-ssh-key\",\"secret\":{\"items\":[{\"key\":\"ssh-privatekey\",\"path\":\"ssh-privatekey\"}],\"secretName\":\"my-ssh-key\"}}]}}}}\n"
    },
    "version": "a57e6e3a195464a8bbbdc1bff3a6f70ed236154d",
    "config": {
      "enabled": true,
      "namespace": "default",
      "app_repository": "git@github.com:kainlite/gitops-operator.git",
      "manifest_repository": "git@github.com:kainlite/gitops-operator-manifests.git",
      "image_name": "kainlite/gitops-operator",
      "deployment_path": "app/00-deployment.yaml"
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
