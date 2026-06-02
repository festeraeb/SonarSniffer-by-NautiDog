$src='C:\Users\thomf\programming\Bagrecovery\dist\WreckHunter2000_Clean'
$dst='C:\Users\thomf\programming\wreckhunter2000'
if (-Not (Test-Path $dst)) { New-Item -ItemType Directory -Path $dst | Out-Null }

Write-Host "Copying clean workspace contents..."
robocopy $src $dst bag_processor frontend frontendgpt ml scripts src /MIR /XD target .git .vs nauticuvs-publish bfscanner* bagfilework recovered 'research optimization-intergration-mostly cesarops' 'Magwork Branch' 'Master Branch' /XF *.lock *.tmp | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with code $LASTEXITCODE" }

$appDir = Join-Path $dst 'sandbox_app'
if (Test-Path $appDir) { Remove-Item -Recurse -Force $appDir }

Write-Host "Creating new sandbox cargo app (using crates.io nauticuvs)..."
cargo new --bin $appDir | Out-Null
Set-Location $appDir

$cargoToml = @"
[package]
name = "sandbox_app"
version = "0.1.0"
edition = "2021"

[dependencies]
nauticuvs = "0.1.2"
ndarray = "0.16"
"@
Set-Content -Path 'Cargo.toml' -Value $cargoToml

$mainRs = @"
use nauticuvs::{CurveletConfig, curvelet_forward_config};
use ndarray::Array2;

fn main() {
    let config = CurveletConfig::new(3).expect("Config init");
    println!("Config OK scales={}", config.scales);
    let x = Array2::<f32>::zeros((16,16));
    let res = curvelet_forward_config(&x, &config).expect("Curvelet forward");
    println!("Computed coeffs scales={}", res.detail.len());
}
"@
Set-Content -Path 'src/main.rs' -Value $mainRs

Write-Host "Building sandbox app..."
cargo build
Write-Host "Running sandbox app..."
cargo run
