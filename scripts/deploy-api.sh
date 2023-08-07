kubectl --context kind-kind create namespace devnet
kubectl --context kind-kind create configmap stacks-devnet-api-conf --from-file=./Config.toml --namespace devnet && \
kubectl --context kind-kind apply -f ./templates/stacks-devnet-api.template.yaml
