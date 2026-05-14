# Workload Identity (AKS) — Admin Steps

This document contains the commands and steps an Azure admin must run to enable per-workload identity (recommended) for accessing Azure Key Vault from pods via the Secrets Store CSI driver.

Prerequisites:
- `az` CLI installed and logged in as a user with **App registration** and **Role assignment** privileges.
- Cluster OIDC issuer URL and the `serviceAccount` you will federate (e.g., `sonarsniffer-sa` in namespace `sonarsniffer`).

Steps:

1) Create an Azure AD App registration:

```bash
az ad app create --display-name "sonarsniffer-workload-identity" \
  --required-resource-accesses []
# Note: `az ad app create` returns appId (the clientId) and objectId. Save both.
```

2) Create a federated credential bound to your Kubernetes service account (replace values):

```bash
az ad app federated-credential create \
  --id <appId> \
  --name "ci-github-aks-federated" \
  --issuer "https://sts.windows.net/<TENANT_ID>/" \
  --subject "system:serviceaccount:sonarsniffer:sonarsniffer-sa" \
  --audiences "api://AzureADTokenExchange"
```

Notes:
- `issuer` should be the cluster OIDC issuer (AKS provides this value via `az aks show -g <rg> -n <cluster> --query "oidcIssuerProfile.issuerUrl" -o tsv`).
- `subject` must be `system:serviceaccount:<namespace>:<service-account-name>`.

3) Assign Key Vault access using Azure RBAC (Key Vault Secrets User role):

```bash
# Use the app's objectId or principalId as the assignee
az role assignment create --role "Key Vault Secrets User" \
  --assignee <appObjectId or appId> \
  --scope "/subscriptions/<sub>/resourceGroups/<rg>/providers/Microsoft.KeyVault/vaults/<vaultName>"
```

4) Update `helm/sonarsniffer/templates/secretproviderclass.yaml` to use workload identity:

Set the following `parameters` in the `SecretProviderClass` for the Azure provider:

```yaml
usePodIdentity: "true"
useVMManagedIdentity: "false"
clientID: "<app-client-id>"
```

5) Apply changes and redeploy the Helm chart:

```bash
helm upgrade --install sonarsniffer ./helm/sonarsniffer -n sonarsniffer --set keyVault.createSecretSyncRBAC=true
```

If you need, I can prepare a PR that adds a `values.yaml` option to switch to workload identity and a script for admins. If you cannot run these commands, provide an admin contact or request they run the script.
