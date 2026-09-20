$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot

try {
    $env:SLINT_ENABLE_EXPERIMENTAL_FEATURES = "1"
    if (-not (Get-Command slint-viewer -ErrorAction SilentlyContinue)) {
        throw "slint-viewer is required for the fast UI check; install a Slint 1.18.x viewer, then retry"
    }
    $viewerVersion = (& slint-viewer --version 2>$null) -join "`n"
    if ($viewerVersion -notmatch "\s1\.18\.") {
        throw "ui-check requires a Slint 1.18.x viewer; found: $viewerVersion"
    }

    & cargo run --quiet --locked -p usher-i18n --example prepare-slint -- target/i18n-dev usher-ui | Out-Null
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    & slint-viewer --check `
        -L "material=material-1.18.0/material.slint" `
        -L "ui-components=ui/features/ui_components_enabled.slint" `
        -L "pr-review=ui/features/pr_review/full.slint" `
        -L "ai-review=ui/features/ai_review/full.slint" `
        -L "i18n=target/i18n-dev/usher-i18n/i18n.slint" `
        "ui/main.slint"
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
finally {
    Pop-Location
}
