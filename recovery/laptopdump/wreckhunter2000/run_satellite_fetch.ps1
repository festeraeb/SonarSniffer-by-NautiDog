# Satellite Target Fetcher - Easy Launcher
# Usage: .\run_satellite_fetch.ps1 [-Bbox <name|coords>] [-StartDate <YYYY-MM-DD>] [-EndDate <YYYY-MM-DD>] [-Output <kml|kmz|both>]

param(
    [Parameter(Mandatory=$false)]
    [string]$Bbox = "corridor",
    
    [Parameter(Mandatory=$false)]
    [string]$StartDate = "",
    
    [Parameter(Mandatory=$false)]
    [string]$EndDate = "",
    
    [Parameter(Mandatory=$false)]
    [ValidateSet("kml", "kmz", "both")]
    [string]$Output = "both"
)

# Conda environment setup
$CONDA_PATH = "C:\Users\thomf\miniconda3\Scripts\conda.exe"
$ENV_NAME = "wreckhunter"
$SCRIPT_PATH = "$PSScriptRoot\satellite_target_fetcher.py"

# Build command arguments
$ARGS = @("run", "-n", $ENV_NAME, "python", $SCRIPT_PATH, "--bbox", $Bbox, "--output", $Output)

if ($StartDate) {
    $ARGS += "--start-date"
    $ARGS += $StartDate
}

if ($EndDate) {
    $ARGS += "--end-date"
    $ARGS += $EndDate
}

# Display configuration
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "SATELLITE TARGET FETCHER - Lake Michigan" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Configuration:" -ForegroundColor Yellow
Write-Host "  Bounding Box: $Bbox" -ForegroundColor White
Write-Host "  Start Date:   $(if($StartDate){$StartDate}else{"(90 days ago)"})" -ForegroundColor White
Write-Host "  End Date:     $(if($EndDate){$EndDate}else{"(today)"})" -ForegroundColor White
Write-Host "  Output:       $Output" -ForegroundColor White
Write-Host ""
Write-Host "Starting fetch..." -ForegroundColor Green
Write-Host ""

# Execute
& $CONDA_PATH $ARGS

# Show output location
Write-Host ""
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "Output files saved to:" -ForegroundColor Yellow
Write-Host "  $PSScriptRoot\outputs\satellite_targets\" -ForegroundColor White
Write-Host ""
Write-Host "Open .kml or .kmz files in Google Earth to view targets." -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Cyan
