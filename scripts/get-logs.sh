kubectl --context kind-kind logs stacks-node --namespace $1 > ./logs/stacks-node.txt & \
kubectl --context kind-kind logs stacks-api --namespace $1 -c stacks-api-container > ./logs/stacks-api.txt & \
kubectl --context kind-kind logs stacks-api --namespace $1 -c stacks-api-postgres > ./logs/stacks-api-postgres.txt & \
kubectl --context kind-kind logs bitcoind-chain-coordinator --namespace $1 -c bitcoind-container > ./logs/bitcoin-node.txt & \
kubectl --context kind-kind logs bitcoind-chain-coordinator --namespace $1 -c chain-coordinator-container > ./logs/chain-coordinator.txt 