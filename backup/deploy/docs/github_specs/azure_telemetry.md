Azure telemetry receiver (runtime errors)

Overview
--------
This repository includes a minimal Azure Function (Python) that accepts POSTed runtime error reports and stores them in an Azure Blob Storage container.

Why use this
------------
- Keeps telemetry inside your Azure tenancy (Azure Educate)
- Storage-backed archive of reports for later analysis
- Easy to swap out for other receivers (Ionos, Sentry) by changing the `SONARSNIFFER_TELEMETRY_URL` env var

Files
-----
- `tools/azure_runtime_error_function/__init__.py` — Azure Function handler
- `tools/azure_runtime_error_function/function.json` — Function binding
- `tools/azure_runtime_error_function/requirements.txt` — Function dependencies

Deployment (quick start)
------------------------
1. Install Azure Functions Core Tools and the Azure CLI:
   - https://learn.microsoft.com/azure/azure-functions/functions-run-local
2. Login and create a function app (python runtime):
   az login
   az group create -n myTelemetryRG -l eastus
   az storage account create -n mytelemetrysa -g myTelemetryRG -l eastus --sku Standard_LRS
   az functionapp create -n my-telemetry-func -g myTelemetryRG -s mytelemetrysa --runtime python --runtime-version 3.10 --functions-version 4
3. Configure environment variables for the Function App:
   - AZURE_STORAGE_CONNECTION_STRING (from the storage account)
   - RUNTIME_ERROR_CONTAINER (optional, defaults to 'runtime-errors')
4. Deploy using `func` or `az functionapp deployment`.

Test locally
------------
- Start function locally:
  func start --script-root tools/azure_runtime_error_function

- POST a test payload:
  curl -X POST http://localhost:7071/api/YourFunctionName -H "Content-Type: application/json" -d '{"test": "payload"}'

Integration with SonarSniffer
----------------------------
- Set `SONARSNIFFER_TELEMETRY_URL` to the function's URL (e.g., `https://<app>.azurewebsites.net/api/<function>`)
- Optionally set `SONARSNIFFER_TELEMETRY_TOKEN` to a function-level key. The telemetry client will include it as a Bearer token header.

Switching later
---------------
- To change backend (e.g., Ionos), update `SONARSNIFFER_TELEMETRY_URL` to your new HTTP receiver; the telemetry client will POST JSON to `<url>/runtime_errors` by default.

CI Telemetry Validation (optional)
----------------------------------
- The repository includes a CI telemetry validation step that runs when two secrets are present:
  - `AZURE_TEST_STORAGE_CONNSTR` — connection string for a throwaway Azure Storage account
  - `AZURE_TEST_STORAGE_CONTAINER` — container name to upload the test payload into
- If these are configured in GitHub repo secrets, CI will run `scripts/ci_azure_telemetry_test.py` to upload a small JSON payload and verify the account/container are writable. This helps validate end-to-end telemetry configuration during PRs.
