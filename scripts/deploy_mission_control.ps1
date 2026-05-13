# Deploy Mission Control to IONOS
# Run this from the root of the repository

Write-Host "Deploying Mission Control to IONOS..." -ForegroundColor Cyan

# Ensure the target directory exists
$targetDir = "tauri/dist-web/mission-control"
if (-not (Test-Path $targetDir)) {
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
}

# Copy the standalone HTML file
Copy-Item -Path "tauri-mission-control/index.html" -Destination "$targetDir/index.html" -Force

Write-Host "File copied to $targetDir/index.html" -ForegroundColor Green

# Run the existing deploy script
Write-Host "Running deploy_web.py --ionos..." -ForegroundColor Yellow
python scripts/deploy_web.py --ionos

Write-Host "Deployment complete!" -ForegroundColor Green
