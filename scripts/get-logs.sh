kubectl logs stacks-node --namespace $1 > ./logs/stacks-node.txt & \
kubectl logs stacks-api --namespace $1 -c stacks-api-container > ./logs/stacks-api.txt & \
kubectl logs stacks-api --namespace $1 -c stacks-api-postgres > ./logs/stacks-api-postgres.txt & \
kubectl logs bitcoind-chain-coordinator --namespace $1 -c bitcoind-container > ./logs/bitcoin-node.txt & \
kubectl logs bitcoind-chain-coordinator --namespace $1 -c chain-coordinator-container > ./logs/chain-coordinator.txt 