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

## Configuration
The `Config.toml` at the root directory of the project can be used to control some settings. This same file can be used to update both the stable and development build. The following settings are supported:
 - `allowed_origins` - this setting is an array of strings and is used to set what origins are allowed in cross-origin requests. For example, `allowed_origins = ["*"]` allows any origins to make requests to this service, while `allowed_origins = ["localhost:3002", "dev.platform.so"]` will only allow requests from the two specified hosts.
 - `allowed_methods` - this setting is an array of strings that sets what HTTP methods can be made to this server.

## Deploying the Stable Version
First, you'll need to use your docker credentials to be able to pull the docker image. To create the needed secret, in your terminal run:
```
kubectl create secret --namespace devnet docker-registry stacks-devnet-api-secret --docker-server=https://index.docker.io/v1/ --docker-username=<user> --docker-email=<email> --docker-password=<password>
```
and enter in the details for a docker user that has access to the `hirosystems/stacks-devnet-api` image.

Then, in your terminal, run
```
./scripts/deploy-api.sh
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

Finally, run 
```
./scripts/redeploy-api.sh
```
to deploy to your local cluster.

## Usage

When the service has been deployed to your Kubernetes cluster, it should be reachable at `localhost:8477`. The following routes are currently exposed:
 - `POST localhost:8477/api/v1/networks` - Creates a new devnet from the configuration provided in request body. See [this example](./examples/new-network.example.json) object for the required parameters If any devnet assets exist when this method is used, no devnet assets will be created, and a 409 error will be returned. **Note: If the namespace for this devnet has not already been created for the cluster, this will fail, unless running a development build (via `cargo run`). A production build expects the namespace to already exist (because the platform should have already created the namespace before creating a devnet). This devnet service should not have permissions to create a namespace. To manually create a namespace, run `kubectl create namespace <namespace>`**
 - `DELETE localhost:8477/api/v1/network/<network-id>` - Deletes all k8s assets deployed under the given namespace. If no devnet assets exist for the given namespace, a 404 error will be returned.
 - `HEAD localhost:8477/api/v1/network/<network-id>` - Checks if any devnet assets exist for the given namespace. If any assets exist, this route responds with 200; if no devnet assets exist, this route responds with 404.
 - `GET localhost:8477/api/v1/network/<network-id>` - Gets the pod and chaintip status for the specified devnet. If not all devnet assets exist for the given namespace, a 404 error will be returned. For example:
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
 - `GET/POST localhost:8477/api/v1/network/<network-id>/stacks-node/*` - Forwards `*` to the underlying stacks node pod of the devnet. If not all devnet assets exist for the given namespace, a 404 error will be returned.
 - `GET/POST localhost:8477/api/v1/network/<network-id>/bitcoin-node/*` - Forwards `*` to the underlying bitcoin node pod of the devnet. If not all devnet assets exist for the given namespace, a 404 error will be returned.
- `GET/POST localhost:8477/api/v1/network/<network-id>/stacks-api/*` - Forwards `*` to the underlying stacks api pod of the devnet. If not all devnet assets exist for the given namespace, a 404 error will be returned.

## Bugs and feature requests

If you encounter a bug or have a feature request, we encourage you to follow the steps below:

 1. **Search for existing issues:** Before submitting a new issue, please search [existing and closed issues](../../issues) to check if a similar problem or feature request has already been reported.
 1. **Open a new issue:** If it hasn't been addressed, please [open a new issue](../../issues/new/choose). Choose the appropriate issue template and provide as much detail as possible, including steps to reproduce the bug or a clear description of the requested feature.
 1. **Evaluation SLA:** Our team reads and evaluates all the issues and pull requests. We are avaliable Monday to Friday and we make a best effort to respond within 7 business days.

Please **do not** use the issue tracker for personal support requests. You'll find help at the [#support Discord channel](https://discord.gg/SK3DxdsP).

## Contribute

Development of this product happens in the open on GitHub, and we are grateful to the community for contributing bugfixes and improvements. Read below to learn how you can take part in improving the product.

### Code of Conduct
Please read our [Code of conduct](../../../.github/blob/main/CODE_OF_CONDUCT.md) since we expect project participants to adhere to it. 

### Contributing Guide
Read our [contributing guide](.github/CONTRIBUTING.md) to learn about our development process, how to propose bugfixes and improvements, and how to build and test your changes.

### Community

Join our community and stay connected with the latest updates and discussions:

- [Join our Discord community chat](https://discord.gg/ZQR6cyZC) to engage with other users, ask questions, and participate in discussions.

- [Visit hiro.so](https://www.hiro.so/) for updates and subcribing to the mailing list.

- Follow [Hiro on Twitter.](https://twitter.com/hirosystems)