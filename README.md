# gitops-operator
[![ci](https://github.com/kainlite/gitops-operator/actions/workflows/ci.yml/badge.svg)](https://github.com/kainlite/gitops-operator/actions/workflows/ci.yml)
[![docker image](https://img.shields.io/docker/pulls/kainlite/gitops-operator.svg)](
https://hub.docker.com/r/kainlite/gitops-operator/tags/)

This was created based in the example [here](https://github.com/kube-rs/version-rs) from [kube-rs](https://github.com/kube-rs)

Basically this is a GitOps controller working in pull mode, triggered by Kubernetes on a readiness check (this is a
hack) to trigger our fake reconcile method, this can be considered a toy controller or learning project.

## Article
You can [read](https://redbeard.team/en/blog/create-your-own-gitops-controller-with-rust) or watch the video here (coming soon tm)... 

### Locally
Run against your current Kubernetes context:

```sh
kind create cluster
## Apply the manifests from the gitops-operator-manifests 
# kustomize build . | kubectl apply -f -
cargo watch
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

## Developing
- Locally against a cluster: `cargo watch`
- In-cluster: edit and `tilt up` [*](https://tilt.dev/)
- Docker build & import to kind: `just build && just import`
