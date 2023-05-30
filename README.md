# Stacks Devnet API
Spins up a server that provides an API to deploy, delete, control, and make requests to Stacks Devnets running on Kubernetes.

## Installation
Running this tool requires having Kubernetes installed. Some [kind](https://kind.sigs.k8s.io/) configuration scripts are included in this repo. Using kind is not a requirement for using this tool, but you will need to use some tool for running local Kubernetes clusters. This is the recommended set of installation instructions for using this tool.
1. [Install Kubernetes.](https://kubernetes.io/releases/download/)
2. [Install `kubectl`.](https://kubernetes.io/releases/download/#kubectl)
3. Install kind.
```
brew install kind
```
4. Create kind cluster
```
./scripts/kind-deploy.sh
```

You should be good to go!

## Usage
Run
```
cargo run
```

to start the server. Currently, the server is hosted on `localhost:8477` and exposes two routes:
 - `POST localhost:8477/api/v1/networks` - Creates a new devnet with configuration provided in request body. See [this example](./examples/new-network.example.json) object for the required parameters.
 - `DELETE localhost:8477/api/v1/network?network={namespace}` - Deletes all k8s assets deployed under the given namespace.

### Notes
This project is still very eary in development and the code is fragile and will change a lot. Some known issues:
 - if a k8s deployment fails, the app crashes. K8s deployments fail for a lot of reasons, so you'll need to restart the service a lot.
 - the project relies on a docker image called `stacks-network`, which is not yet deployed to docker hub. This is in progress.