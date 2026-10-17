# TaskManager — working charter

TaskManager is a native task manager and system monitor for macOS, Windows and Linux,
built on agg-gui and shipped native plus WebAssembly (a GitHub Pages demo on a simulated
machine). It is a clean-room implementation of the *design* of TMOG (tmog.org).
`docs/PLAN.md` is the spec: the clean-room rules, every screen, the instrument widgets,
the telemetry architecture and milestones (M0–M13). Read it before writing code, and keep
it current when a decision changes.

## The pillars

**Clean room.** Our only inputs are the screenshots in `reference/` (gitignored, see
`reference/README.md`), the public website copy and using TMOG as a user. Never
decompile, extract resources from, or copy text from TMOG; never use its name, logo or
wordmark in the product. We have no Pro tier: no upgrade banners, licence keys or SAMPLE
watermarks. Themes are agg-gui's Light and Dark (plus Match System) only — no phosphor
themes, no popout skins.

**Match the references.** Each page is done when it looks close to its reference capture
(layout, proportions, type, colours, meter and graph look), checked side by side with
`cargo run -p taskmanager-core --example compare_refs`. Not pixel perfect; close.

**A measuring instrument never makes up an answer.** Every reading is a `Reading<T>`
(`Value` / `Unavailable` / `Unsupported`). Never display a zero you did not measure;
never draw a line through missing samples; never fake a sensor. Actions on processes
take a `ProcessKey` (pid + start time) and re-check it before acting.

**No stubs, no shortcuts.** Every function is complete and production-ready. No `todo!()`,
no `unimplemented!()`, no placeholder screens or partial implementations. If a dependency
isn't ready, build it first.

**Test-first bug fixing.** 1) Write a failing test that reproduces the bug. 2) Fix it.
3) Confirm the test passes. Never commit a bug fix that isn't covered by a test. Never
weaken, `#[ignore]` or delete a test to make it pass.

**All UI through agg-gui.** The shells (`taskmanager-native`, `taskmanager-wasm`) create
the window or canvas, supply platform services and get out of the way: no widget
construction, no user-facing strings, no HTML/CSS UI beyond the bare canvas. Use
agg-gui's stock widgets (`Table`, `TreeView`, `Splitter`, `ComboBox`, `ToggleSwitch`,
`Slider`, `ScrollView`, `Toasts`, …); custom widgets are the instruments in
`taskmanager-core/src/ui/instruments/` and anything agg-gui genuinely lacks.

**Core stays platform-free.** No `cfg(target_os)` in `taskmanager-core`. OS code lives in
`taskmanager-native/src/platform/{macos,windows,linux}.rs`; pure parsers for OS text
formats live in the core (`platform/parse/`) so they are tested on every OS. Widgets ask
the platform (`is_demo`), never `cfg`.

**Icons are Font Awesome** (4.7, agg-gui's bundled face) by code point, named in
`taskmanager-core/src/fonts.rs` (`fa::*`). Text uses agg-gui's default font (titles just set
large). No image icons except per-app icons read from the OS.

## Local development uses agg-gui as a path dep — improve it as you go

`Cargo.toml` patches `agg-gui` and `agg-gui-shell` to the sibling checkout `../agg-gui/`,
and `agg-gui-web-shell` is a path dependency there. When TaskManager needs a capability
agg-gui lacks (a banner, a table cell painter, a glow effect), add it to agg-gui itself
(then Lars publishes a new version) — never a local workaround. CI clones agg-gui beside
the repo so the paths resolve there too.

## Build and test

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --package taskmanager-core --package taskmanager-native --package taskmanager-wasm
cargo run -p taskmanager-native [-- --demo]
cargo check -p taskmanager-wasm --target wasm32-unknown-unknown
```

`TASKMANAGER_SCREENSHOT=out.png [TASKMANAGER_PAGE=summary] [TASKMANAGER_THEME=dark]
cargo run -p taskmanager-native -- --demo` renders one frame and exits.
`cargo run -p taskmanager-core --example screenshot_pages -- out/` renders every page
headless from the seeded demo machine; `--example compare_refs` writes side-by-side
comparisons with `reference/` to `target/compare/`.
Run `scripts/pre-commit-check.sh` (or `.ps1`) before every commit; it mirrors CI.
On macOS, `run-mac.command` builds the release binary and launches it; on Windows,
`run-windows.cmd`; on Linux, `run-linux.sh`.

## Workflow: implementer → reviewer, commit as you go

The orchestrating session splits a milestone into small steps and hands each to the
`implementer` subagent with exact file boundaries, the PLAN section and the reference
image; then the `reviewer` subagent reviews the diff against PLAN.md, this charter and
the reference. Fix what the review finds, then commit. One logical change per commit, as
soon as it is green (fmt, clippy `-D warnings`, tests). Push `main` when a milestone step
lands, then watch CI (`gh run list`) and fix anything red before moving on.

## Performance

Never guess: measure first (`TASKMANAGER_STATS=1` prints sampler, layout and paint
times). The monitor must be cheap: idle CPU under 1 % of one core at the default 1 s
interval with 60 fps visuals off, and average frame time under 10 ms. Each optimisation
must show up in the numbers or it is reverted.

## Coding standards

- Files stay at or below 800 lines (`file_line_count` test). Split into real modules;
  never squeeze under by deleting comments or blank lines.
- Every file starts with a comment block saying what it is for and how it relates to the
  rest of the project.
- No `unwrap()` outside `main` and tests; prefer `Result` / `Option`. `expect` is fine in
  `main` for a start-up failure with a clear message.
- No `unsafe` unless there is no alternative (OS FFI); document every `unsafe` block.
- Rust naming conventions; doc comments where they clarify intent.
