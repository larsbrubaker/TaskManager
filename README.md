# TaskManager

A native task manager and system monitor for macOS, Windows and Linux, built in Rust on
[agg-gui](https://github.com/larsbrubaker/agg-gui). It shows what your machine is doing
(CPU, memory, disks, network, GPU, power and thermals, processes, services, startup apps,
connections and more) with live meters and history graphs, and every reading is either
measured or clearly marked as unavailable: it never shows a number it did not measure.
A WebAssembly build will run as a demo on a simulated machine.

TaskManager is a clean-room implementation of the design of
[TMOG](https://tmog.org) (Task Manager OG); it contains no TMOG code. See
[`docs/PLAN.md`](docs/PLAN.md) for the clean-room rules, the screens and the milestones.

## Status

Early development (milestone M0, the scaffold). The window opens; the screens are being
built one milestone at a time.

## Build from source

You need a Rust toolchain (1.95 or newer). The workspace expects
[agg-gui](https://github.com/larsbrubaker/agg-gui) and its own path dependencies,
[agg-rust](https://github.com/larsbrubaker/agg-rust) and
[tess2-rust](https://github.com/larsbrubaker/tess2-rust), checked out next to this
repository (`../agg-gui`, `../agg-rust`, `../tess2-rust`), as they are in the `rust-apps`
superproject:

```sh
git clone https://github.com/larsbrubaker/TaskManager.git
git clone https://github.com/larsbrubaker/agg-gui.git
git clone https://github.com/larsbrubaker/agg-rust.git
git clone https://github.com/larsbrubaker/tess2-rust.git
cd TaskManager
cargo run --release
```

Or use the launchers, which build a release binary and start it: double-click
`run-mac.command` on macOS or `run-windows.cmd` on Windows, or run `./run-linux.sh` on
Linux.

Checks (the same as CI): `scripts/pre-commit-check.sh` (or `.ps1` on Windows).

## License

MIT; see [LICENSE](LICENSE).
