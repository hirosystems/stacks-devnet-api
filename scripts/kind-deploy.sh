kind create cluster --config=./templates/initial-config/kind.yaml && \
docker pull hirosystems/stacks-blockchain-api:latest --platform=linux/amd64 && \
kind load docker-image hirosystems/stacks-blockchain-api && \
kind load docker-image stacks-network && \
kubectl --context kind-kind apply -f https://openebs.github.io/charts/openebs-operator.yaml && \
kubectl apply -f ./templates/initial-config/storage-class.yaml