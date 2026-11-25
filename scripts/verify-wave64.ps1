param([switch]$Benchmark)

$ErrorActionPreference = 'Stop'

function Invoke-CargoCheck {
    param([string[]]$Arguments)
    Write-Host ("cargo " + ($Arguments -join " "))
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw ("cargo failed with exit code " + $LASTEXITCODE)
    }
}

Push-Location (Join-Path $PSScriptRoot '..')
try {
    Invoke-CargoCheck @('clippy', '--workspace', '--all-targets', '--all-features', '--locked', '--', '-D', 'warnings')
    Invoke-CargoCheck @('test', '-p', 'victory-shader', '--lib', '--locked', 'wave64_', '--', '--test-threads=1', '--nocapture')
    Invoke-CargoCheck @('test', '--workspace', '--locked', '--', '--test-threads=1')
    Invoke-CargoCheck @('build', '--release', '-p', 'victory', '--locked')
    if ($Benchmark) {
        Invoke-CargoCheck @('test', '--release', '-p', 'victory-shader', '--lib', '--locked', 'benchmark_wave64_portable_exchange', '--', '--ignored', '--test-threads=1', '--nocapture')
    }
    Write-Host 'All requested validation commands completed successfully.'
} finally {
    Pop-Location
}
