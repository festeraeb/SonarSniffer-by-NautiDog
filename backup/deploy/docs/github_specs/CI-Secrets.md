# CI Secrets — what to add to GitHub

The `ci-deploy.yml` workflow requires these repository **secrets** (add via Settings → Secrets → Actions):

- `AZURE_CREDENTIALS` — JSON output from `az ad sp create-for-rbac --sdk-auth` or an equivalent service principal credential with access to the subscription/AKS/ACR resource group.
- `ACR_LOGIN_SERVER` — e.g. `shipwreckacr.azurecr.io`
- `ACR_USERNAME` and `ACR_PASSWORD` — username and password for ACR (or use `AZURE_CREDENTIALS` if you use `azure/docker-login@v1` without username/password).

Recommended workflow:
1. Create a service principal with `az ad sp create-for-rbac --name "sonarsniffer-ci" --role contributor` (or narrower scope) and save the `--sdk-auth` JSON as `AZURE_CREDENTIALS`.
2. Add the ACR login server and credentials (or use `AZURE_CREDENTIALS` only and the action `azure/docker-login@v1` will handle auth).
3. Push a test commit to `main` and verify the `CI & Deploy` job runs successfully.

If you want, I will prepare a PR that documents these steps and wires the workflow to use the minimal required roles. You said you will add GitHub secrets yourself — let me know when they're in place and I will trigger a test run.
