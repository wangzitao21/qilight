param([switch]$CheckOnly)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot

function Invoke-Cargo {
    param([string[]]$CargoArguments)
    & cargo @CargoArguments
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed: $CargoArguments" }
}

Push-Location -LiteralPath $projectRoot
try {
    # Use the local portable toolchain when available, or an installed Rust toolchain.
    if (Test-Path -LiteralPath '.tools\cargo\bin\cargo.exe') {
        $env:CARGO_HOME = Join-Path $projectRoot '.tools\cargo'
        $env:RUSTUP_HOME = Join-Path $projectRoot '.tools\rustup'
        $env:PATH = "$projectRoot\.tools\cargo\bin;$projectRoot\.tools\w64devkit\bin;$env:PATH"
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $projectRoot '.tools\w64devkit\bin\gcc.exe'
    }
    Invoke-Cargo -CargoArguments @('fmt', '--check')
    Invoke-Cargo -CargoArguments @('test', '--locked')
    Invoke-Cargo -CargoArguments @('clippy', '--locked', '--all-targets', '--', '-D', 'warnings')
    if ($CheckOnly) { return }

    $metadata = Invoke-Cargo -CargoArguments @('metadata', '--locked', '--no-deps', '--format-version', '1') | ConvertFrom-Json
    $package = $metadata.packages | Where-Object { $_.name -eq 'qilight' }
    if (-not $package) { throw 'QiLight package metadata is missing.' }

    $toolchain = Invoke-Cargo -CargoArguments @('-vV')
    if ($toolchain -notcontains 'host: x86_64-pc-windows-gnu' -and $toolchain -notcontains 'host: x86_64-pc-windows-msvc') {
        throw 'Building this release requires an x64 Windows Rust toolchain.'
    }
    Invoke-Cargo -CargoArguments @('build', '--release', '--locked')
    $dist = Join-Path $projectRoot 'dist'
    New-Item -ItemType Directory -Path $dist -Force | Out-Null
    $executable = Join-Path $dist 'QiLight.exe'
    Copy-Item -LiteralPath (Join-Path $metadata.target_directory 'release\qilight.exe') -Destination $executable -Force
    $archive = Join-Path $dist "QiLight-$($package.version)-windows-x64.zip"
    # Explicitly package only the executable; local pairing data never enters a release.
    Compress-Archive -LiteralPath $executable -DestinationPath $archive -Force
    Write-Host "Built $executable"
    Write-Host "Packaged $archive"
} finally {
    Pop-Location
}
