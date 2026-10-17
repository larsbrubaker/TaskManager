# Changelog

All notable changes to TaskManager are documented here.

## Unreleased

### Added
- The workspace: `taskmanager-core` (readings, formatting, fonts and icons, widgets),
  `taskmanager-native` (the `taskmanager` desktop binary on agg-gui-shell) and
  `taskmanager-wasm` (the browser shell on agg-gui-web-shell).
- A window titled TaskManager (1242 x 791 by default, at least 1100 x 700);
  `TASKMANAGER_SCREENSHOT=out.png` captures one frame and exits.
- `Reading<T>` (a measured value, or Unavailable / Unsupported) and the formatting of
  sizes, rates, percentages, temperatures and durations.
- CI on Linux, macOS and Windows plus a wasm32 check; `scripts/pre-commit-check.{sh,ps1}`;
  launchers `run-mac.command`, `run-windows.cmd` and `run-linux.sh`.
