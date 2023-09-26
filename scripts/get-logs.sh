kubectl logs stacks-blockchain --namespace $1 > ./logs/stacks-blockchain.txt & \
kubectl logs stacks-blockchain-api --namespace $1 -c stacks-blockchain-api > ./logs/stacks-blockchain-api.txt & \
kubectl logs stacks-blockchain-api --namespace $1 -c postgres > ./logs/stacks-blockchain-api-pg.txt & \
kubectl logs bitcoind-chain-coordinator --namespace $1 -c bitcoind > ./logs/bitcoin-node.txt & \
kubectl logs bitcoind-chain-coordinator --namespace $1 -c chain-coordinator > ./logs/chain-coordinator.txt 