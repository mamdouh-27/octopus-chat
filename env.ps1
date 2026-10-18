# Octopus Chat - PowerShell build environment (optional)
#
# Reads build-env.json if present so these paths and the npm scripts
# (`scripts/run.mjs`) can never drift apart. Dot-source before building by hand:
#
#   . .\env.ps1
#   cargo test
#
# On a machine with a normal system drive there is no build-env.json and this
# script does nothing. `npm run dev`, `npm run build` and `npm run tauri ...`
# load it automatically.

$envPath = Join-Path $PSScriptRoot "build-env.json"
if (Test-Path $envPath) {
  $cfg = Get-Content $envPath -Raw | ConvertFrom-Json

  $env:RUSTUP_HOME = Join-Path $cfg.buildRoot "rustup"
  $env:CARGO_HOME = Join-Path $cfg.buildRoot "cargo"
  $env:CARGO_TARGET_DIR = Join-Path $cfg.buildRoot "target"
  $env:TEMP = Join-Path $cfg.buildRoot "tmp"
  $env:TMP = Join-Path $cfg.buildRoot "tmp"
  if ($cfg.buildJobs) { $env:CARGO_BUILD_JOBS = "$($cfg.buildJobs)" }
  # The GNU toolchain on Windows needs an explicit linker; cargo picks this up
  # via -C linker=... . No CARGO_BUILD_TARGET: forcing a target triple would
  # move output to target/<triple>/release and invalidate the warm cache.
  if ($cfg.linker) { $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $cfg.linker }

  # The Rust toolchain plus the MSYS2 UCRT64 linker used by x86_64-pc-windows-gnu.
  $env:PATH = "$env:CARGO_HOME\bin;$($cfg.extraPath -join ';');$env:PATH"
}