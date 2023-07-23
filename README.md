# Stacks Devnet API
Spins up a server that provides an API to deploy, delete, control, and make requests to Stacks Devnets running on Kubernetes.

## Prerequisites
Running this tool requires having Kubernetes installed. Some [kind](https://kind.sigs.k8s.io/) configuration scripts are included in this repo. Using kind is not a requirement for using this tool, but you will need to use some tool for running local Kubernetes clusters. This is the recommended set of installation instructions for using this tool.
1. [Install Kubernetes.](https://kubernetes.io/releases/download/)
2. [Install `kubectl`.](https://kubernetes.io/releases/download/#kubectl)
3. [Install Docker Desktop.](https://docs.docker.com/desktop/install/mac-install/)
4. Install kind.
```
brew install kind
```
5. With Docker Desktop running, create kind cluster
```
./scripts/kind-deploy.sh
```

You should now be ready to deploy this service to your local Kubernetes cluster!

## Deploying the Stable Version
In your terminal, rum 
```
kubectl --context kind-kind apply -f ./templates/stacks-devnet-api.template.yaml
```
to install the [latest version of this service](https://quay.io/repository/hirosystems/stacks-devnet-api?tab=history) that has been deployed to docker (or, to quay for now). This service should now be fully running on your Kubernetes cluster. See the [usage](#usage) sections for steps on how to use the service.

## Deploying a Development Build
Any changes made to this codebase can be tested, in part, by running `cargo run`. However, some features won't be available when running the service this way. Some of the inter-pod communication that takes place requires connected services to be running _in_ Kubernetes.

To deploy a local version of this tool to you Kubernetes cluster, create a docker build and load it to your kind cluster:
```
docker build -t stacks-devnet-api . && kind load docker-image stacks-devnet-api
```
Now, modify the `stacks-devnet-api.template.yaml` file to deploy this new, local version rather than the latest stable version:
```diff
apiVersion: v1
kind: Pod
metadata:
  labels:
    name: stacks-devnet-api
  name: stacks-devnet-api
  namespace: devnet
spec:
  serviceAccountName: stacks-devnet-api-service-account
  containers:
  - command:
    - ./stacks-devnet-api
    name: stacks-devnet-api-container
-    image: quay.io/hirosystems/stacks-devnet-api:latest
-    imagePullPolicy: Always
+    image: stacks-devnet-api:latest
+    imagePullPolicy: Never
```

If a version of this tool has already been deployed to your local cluster, you'll need to delete the existing pod. You'll need to do this every time you redeploy the service:
```
kubectl --context kind-kind delete pod stacks-devnet-api --namespace devnet
```

Finally, run 
```
kubectl --context kind-kind apply -f ./templates/stacks-devnet-api.template.yaml
```
to deploy to your local cluster.

## Usage

When the service has been deployed to your Kubernetes cluster, it should be reachable at `localhost:8477`. The following routes are currently exposed:
 - `POST localhost:8477/api/v1/networks` - Creates a new devnet from the configuration provided in request body. See [this example](./examples/new-network.example.json) object for the required parameters. **Note: If the namespace for this devnet has not already been created for the cluster, this will fail, unless running a development build (via `cargo run`). A production build expects the namespace to already exist (because the platform should have already created the namespace before creating a devnet). This devnet service should not have permissions to create a namespace. To manually create a namespace, run `kubectl create namespace <namespace>`**
 - `DELETE localhost:8477/api/v1/network/<network-id>` - Deletes all k8s assets deployed under the given namespace.
 - `GET localhost:8477/api/v1/network/<network-id>` - Gets the pod and chaintip status for the specified devnet. For example:
```JSON
{
    "bitcoind_node_status": "Running",
    "stacks_node_status": "Running",
    "stacks_api_status": "Running",
    "bitcoind_node_started_at": "2023-07-11 00:30:02 UTC",
    "stacks_node_started_at": "2023-07-11 00:30:07 UTC",
    "stacks_api_started_at": "2023-07-11 00:30:11 UTC",
    "stacks_chain_tip": 14,
    "bitcoin_chain_tip": 116
}
```
 - `GET/POST localhost:8477/api/v1/network/<network-id>/stacks-node/*` - Forwards `*` to the underlying stacks node pod of the devnet.
 - `GET/POST localhost:8477/api/v1/network/<network-id>/bitcoin-node/*` - Forwards `*` to the underlying bitcoin node pod of the devnet.
- `GET/POST localhost:8477/api/v1/network/<network-id>/stacks-api/*` - Forwards `*` to the underlying stacks api pod of the devnet.