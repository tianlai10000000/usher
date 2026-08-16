$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot

try {
    $env:SLINT_ENABLE_EXPERIMENTAL_FEATURES = "1"
    if (-not (Get-Command slint-viewer -ErrorAction SilentlyContinue)) {
        throw "slint-viewer is required for the fast UI check; install the Slint 1.17.1 viewer, then retry"
    }

    & slint-viewer --check `
        -L "material=material-1.7.1-fixed/material.slint" `
        -L "ui-components=ui/features/ui_components_enabled.slint" `
        -L "pr-review=ui/features/pr_review/full.slint" `
        "ui/main.slint"
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
finally {
    Pop-Location
}
