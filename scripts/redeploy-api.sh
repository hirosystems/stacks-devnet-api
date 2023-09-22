kubectl --context kind-kind delete configmap stacks-devnet-api --namespace devnet & \
kubectl --context kind-kind delete pod stacks-devnet-api --namespace devnet && \
./scripts/deploy-api.sh
