$source='C:\Users\thomf\programming\Bagrecovery\dist\WreckHunter2000_Clean'
$dst='C:\Users\thomf\programming\wreckhunter2000\databases'
if(-not(Test-Path $dst)){New-Item -ItemType Directory -Path $dst | Out-Null}
$dbs=@('wreck_hunting_ml\\data\\shipwrecks.db','wreck_hunting_ml\\data\\wh2k.db','bag_processor\\db\\wrecks.db','bag_processor\\survey_inventory.sqlite')
foreach($f in $dbs){
    $src=Join-Path $source $f
    if(Test-Path $src){Copy-Item -Path $src -Destination $dst -Force; Write-Host "Copied $f"} else {Write-Host "Missing $f"}
}
Get-ChildItem -Path $dst -Include *.db,*.sqlite,*.sqlite3 -File | ForEach-Object {
    Write-Host "DB: $($_.FullName)"
    if(Get-Command sqlite3 -ErrorAction SilentlyContinue){
        sqlite3 $_.FullName '.tables' | Write-Host
    } else {
        Write-Host 'sqlite3 not available'
    }
}
# run command tests one by one with 60s timeout
$commands = @(
    'python "C:\Users\thomf\programming\wreckhunter2000\bag_processor\bag_auto_pipeline.py" --help',
    'python "C:\Users\thomf\programming\wreckhunter2000\bag_processor\advanced_bag_scanner_runner.py" --help',
    'python "C:\Users\thomf\programming\wreckhunter2000\bag_processor\comprehensive_bag_gui.py" --help'
)
foreach($c in $commands){
    Write-Host "Running: $c"
    $p = Start-Process -FilePath powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-Command',$c -PassThru -WindowStyle Hidden
    if(-not $p.WaitForExit(60000)){
        $p.Kill()
        Write-Host "Timeout: $c"
    } else {
        Write-Host "Exit $($p.ExitCode): $c"
    }
}
