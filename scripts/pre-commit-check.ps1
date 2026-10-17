# Pre-commit checks for TaskManager. Run from the repo root.
# Mirrors CI: file-length guardrail, tests, fmt, clippy, wasm check.

$ErrorActionPreference = "Continue"
$failed = $false

Write-Host "==> file length guardrail" -ForegroundColor Cyan
cargo test -p taskmanager-core --test file_line_count --quiet
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host "==> cargo test --workspace" -ForegroundColor Cyan
cargo test --workspace --quiet
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host "==> cargo fmt --check" -ForegroundColor Cyan
cargo fmt --package taskmanager-core --package taskmanager-native --package taskmanager-wasm -- --check
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host "==> cargo clippy" -ForegroundColor Cyan
cargo clippy --workspace --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host "==> cargo check (wasm32)" -ForegroundColor Cyan
cargo check -p taskmanager-wasm --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { $failed = $true }

Write-Host "==> cargo clippy (wasm32)" -ForegroundColor Cyan
cargo clippy -p taskmanager-wasm --target wasm32-unknown-unknown -- -D warnings
if ($LASTEXITCODE -ne 0) { $failed = $true }

if ($failed) {
    Write-Host "PRE-COMMIT CHECKS FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "All pre-commit checks passed" -ForegroundColor Green
