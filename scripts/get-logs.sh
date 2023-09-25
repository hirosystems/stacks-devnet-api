kubectl --context kind-kind logs stacks-blockchain --namespace $1 > ./logs/stacks-blockchain.txt & \
kubectl --context kind-kind logs stacks-blockchain-api --namespace $1 -c stacks-blockchain-api > ./logs/stack-blockchains-api.txt & \
kubectl --context kind-kind logs stacks-blockchain-api --namespace $1 -c postgres > ./logs/stacks-blockchain-api-pg.txt & \
kubectl --context kind-kind logs bitcoind-chain-coordinator --namespace $1 -c bitcoind > ./logs/bitcoin-node.txt & \
kubectl --context kind-kind logs bitcoind-chain-coordinator --namespace $1 -c chain-coordinator > ./logs/chain-coordinator.txt 