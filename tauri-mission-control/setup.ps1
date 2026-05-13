# CESAROPS Mission Control Tauri Setup Script
# Run this in PowerShell from the tauri-mission-control directory

Write-Host "Installing Node.js dependencies..." -ForegroundColor Cyan
npm install

Write-Host "Installing Tauri CLI globally..." -ForegroundColor Cyan
npm install -g @tauri-apps/cli

Write-Host "Generating Tauri configuration..." -ForegroundColor Cyan
npx tauri init

Write-Host "Building the app..." -ForegroundColor Cyan
npx tauri build

Write-Host "Build complete! Check the target/release directory." -ForegroundColor Green
```

---

## 2. Web Deployment to IONOS

This places the standalone Mission Control HTML into the `dist-web` folder, ready for the existing deploy script. It configures the backend to point to the public tunnel (`api.cesarops.org`).

###
