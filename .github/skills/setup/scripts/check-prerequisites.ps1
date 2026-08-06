# Setup — Prerequisite Check (Windows)
# This script checks for required development tools and reports their status.

$tools = @(
    @{ Name = "Rust (rustc)"; Command = "rustc"; Args = "--version"; MinMajor = 1 },
    @{ Name = "cargo";        Command = "cargo"; Args = "--version"; MinMajor = 1 },
    @{ Name = "Git";          Command = "git";   Args = "--version"; MinMajor = 2 },
    @{ Name = "GitHub CLI (gh)"; Command = "gh"; Args = "--version"; MinMajor = 2 }
)

$allGood = $true

foreach ($tool in $tools) {
    $name = $tool.Name
    try {
        $output = & $tool.Command $tool.Args 2>&1
        if ($LASTEXITCODE -ne 0 -and $null -eq $output) {
            throw "not found"
        }
        # Extract version number
        $versionMatch = [regex]::Match($output, '(\d+\.\d+\.\d+)')
        if ($versionMatch.Success) {
            $version = $versionMatch.Value
            $major = [int]($version.Split('.')[0])
            if ($major -ge $tool.MinMajor) {
                Write-Host "  $name v$version" -ForegroundColor Green
            } else {
                Write-Host "  $name v$version (minimum v$($tool.MinMajor)+ required)" -ForegroundColor Yellow
                $allGood = $false
            }
        } else {
            Write-Host "  $name (installed, version unknown)" -ForegroundColor Yellow
        }
    }
    catch {
        Write-Host "  $name - not found" -ForegroundColor Red
        $allGood = $false
    }
}

Write-Host ""
if ($allGood) {
    Write-Host "All prerequisites are installed." -ForegroundColor Green
} else {
    Write-Host "Some prerequisites are missing or outdated. See above." -ForegroundColor Yellow
}
