$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot

try {
    & cargo check `
        --locked `
        --features "dev-shell" `
        --target-dir "target/dev-shell" `
        @args
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
finally {
    Pop-Location
}
