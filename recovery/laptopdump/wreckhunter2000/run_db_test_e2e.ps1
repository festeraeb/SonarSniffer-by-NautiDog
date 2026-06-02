param(
    [int]$timeoutSeconds = 300,
    [string]$sourceRoot = 'C:\Users\thomf\programming\Bagrecovery\dist\WreckHunter2000_Clean',
    [string]$sandboxRoot = 'C:\Users\thomf\programming\wreckhunter2000'
)

$logFile = Join-Path $sandboxRoot 'sandbox_db_test_log.txt'
$missingFileLog = Join-Path $sandboxRoot 'sandbox_missing_db_paths.txt'
"===== DB TEST RUN START $(Get-Date) =====" | Out-File -FilePath $logFile -Append
"===== DB MISSING PATHS =====" | Out-File -FilePath $missingFileLog -Append

function Log{ param($s); "$((Get-Date).ToString('o')) `t $s" | Out-File -FilePath $logFile -Append }
function LogMissing{ param($s); "$((Get-Date).ToString('o')) `t $s" | Out-File -FilePath $missingFileLog -Append }

function Run-CommandWithTimeout {
    param(
        [string]$cmd,
        [int]$timeout
    )
    Log "Start command: $cmd"
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = 'powershell'
    $psi.Arguments = "-NoProfile -ExecutionPolicy Bypass -Command \"$cmd\""
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false

    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $psi
    $p.Start() | Out-Null
    if (-not $p.WaitForExit($timeout * 1000)) {
        $p.Kill()
        Log "Timeout reached ($($timeout) sec) for command: $cmd"
        return @{Success=$false; Output='Timeout'; Error='Timeout'}
    }

    $out = $p.StandardOutput.ReadToEnd()
    $err = $p.StandardError.ReadToEnd()

    if ($p.ExitCode -ne 0) {
        Log "Command failed: $cmd`n$err"
        return @{Success=$false; Output=$out; Error=$err}
    }

    Log "Command succeeded: $cmd`n$out"
    return @{Success=$true; Output=$out}
}

# 1) ensure sandbox db directory
$dbDest = Join-Path $sandboxRoot 'databases'
if (-not (Test-Path $dbDest)) { New-Item -ItemType Directory -Path $dbDest | Out-Null }

# 2) copy known DB files from clean source if they exist
$dbFiles = @(
    'wreck_hunting_ml\data\shipwrecks.db',
    'wreck_hunting_ml\data\wh2k.db',
    'bag_processor\db\wrecks.db',
    'bag_processor\survey_inventory.sqlite'
)
foreach ($rel in $dbFiles) {
    $src = Join-Path $sourceRoot $rel
    if (Test-Path $src) {
        Copy-Item -Path $src -Destination $dbDest -Force
        Log "Copied DB $rel to sandbox databases"
    } else {
        LogMissing "Missing expected DB source file: $src"
    }
}

# 3) find additional DB path references in code to inspect and add to missing log
$patterns = '.*(\S+\.db)|(\S+\.sqlite)|database_path|DB_PATH' 
$searchFiles = Get-ChildItem -Path $sourceRoot -Recurse -Include *.py,*.yaml,*.yml,*.toml,*.json,*.rs -ErrorAction SilentlyContinue
foreach ($f in $searchFiles) {
    try {
        $text = Get-Content $f -ErrorAction Stop
    } catch { continue }
    foreach ($line in $text) {
        if ($line -match $patterns) {
            if ($line -match '(["\']?)([^"\'\s]+\.(db|sqlite|sqlite3))\1') {
                $dbpath = $matches[2]
                # skip generic if relative and default like 'wrecks.db'
                if ($dbpath -notmatch '^(?:\.|\\|/|[A-Za-z]:)') { continue }
                Log "Found DB path reference in $($f.FullName): $dbpath"
                if (-not (Test-Path $dbpath) -and -not (Test-Path (Join-Path $sandboxRoot $dbpath))) {
                    LogMissing "Unresolved DB reference $dbpath in file $($f.FullName)"
                }
            }
        }
    }
}

# 4) run proactive sqlite checks on sandbox DB file(s)
Get-ChildItem -Path $dbDest -Include *.db,*.sqlite,*.sqlite3 -File | ForEach-Object {
    $path = $_.FullName
    if (Get-Command sqlite3 -ErrorAction SilentlyContinue) {
        $cmd = "sqlite3 `"$path`" `.tables`"
        $r = Run-CommandWithTimeout -cmd $cmd -timeout $timeoutSeconds
        if ($r.Success -eq $false) { LogMissing "Could not introspect $path: $($r.Error)" }
    } else {
        Log "sqlite3 not installed; skipping .tables for $path"
    }
}

# 5) execute key test entrypoints (CLI and GUI)
$testCommands = @(
    "python $sandboxRoot\bag_processor\bag_auto_pipeline.py --help",
    "python $sandboxRoot\bag_processor\advanced_bag_scanner_runner.py --help",
    "python $sandboxRoot\bag_processor\comprehensive_bag_gui.py --help"
)
foreach ($cmd in $testCommands) {
    $result = Run-CommandWithTimeout -cmd $cmd -timeout $timeoutSeconds
    if ($result.Success -eq $false) {
        LogMissing "Test command failed: $cmd - $($result.Error)"
    }
}

"===== DB TEST RUN END $(Get-Date) =====" | Out-File -FilePath $logFile -Append
"===== DB MISSING PATHS END $(Get-Date) =====" | Out-File -FilePath $missingFileLog -Append
