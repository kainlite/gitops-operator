docker_build('kainlite/gitops-operator:local', '.', dockerfile='Dockerfile')
local_resource('import', 'just import')
k8s_yaml('deployment.yaml')
k8s_resource('gitops-operator', port_forwards=8000)
