# TaskManager — Plan

A native task manager and system monitor for macOS, Windows and Linux, built on agg-gui,
shipped native plus a WebAssembly demo on a simulated machine. It is a **clean-room
implementation of the design of TMOG** (Task Manager OG, tmog.org, by Dave Plummer): we
reproduce its screens closely by looking at them, and nothing else. We never read,
decompile or reuse TMOG code, and we ship under our own name, logo and copy.

Status (2026-10-17, paused): **M0 done; M1.1–M1.3 done** (telemetry core, demo platform,
native platform). **Resume at M1.4**: run the sampler in the app (native worker thread →
Arc<Snapshot> via ui_thread; wasm steps the demo Sampler), show "providers healthy · N
processes · Generation G" in the status bar, `--demo` flag, and a shared sensor display-name
source (MachineInfo sensor list) for both platforms. First Linux/Windows CI run of the native
code was pending at pause.

License MIT. Repo github.com/larsbrubaker/TaskManager (rust-apps submodule).

The visual targets live in `reference/` (gitignored, see `reference/README.md`): captures
of TMOG 1.0.x running on Lars's Mac (light theme, real data) are the primary reference;
tmog.org screenshots are secondary. Our Dark theme is our own (agg-gui Dark plus our channel
palette), not a copy of TMOG's dark look. "Close, not pixel perfect": same layout,
proportions, type, colours and meter/graph look; data and exact pixels may differ.

---

## 1. Ground rules for the clean room

- **Design only.** Inputs are screenshots, the public website copy and using the app as
  a user. No disassembly, no resource extraction, no copying of TMOG's strings beyond
  ordinary UI words ("Processes", "Memory usage"), no TMOG wordmark or logo.
- **Our own identity.** Product name *TaskManager*; our own logo (three flat LED meter columns in
  the CPU, memory and GPU channel colours beside the word; deliberately not a glowing
  heartbeat like TMOG's mark); our own app icon.
- **No Pro tier.** Every one of TMOG's pages is a live, free feature in ours. There is no
  upgrade banner, licence key, "Get PRO Version" link or SAMPLE watermark.
- **No phosphor themes, no popout skins.** We ship agg-gui's **Light** and **Dark**
  themes plus *Match System*. Instrument colours (CPU green, memory magenta, GPU blue,
  energy yellow, thermals orange, NPU red/amber) are fixed per channel and tuned per
  theme.
- **Instrument, not ornament.** We adopt TMOG's published measurement principles because
  they are good engineering, not because they are TMOG's: every reading carries an
  availability state; we never show a zero we did not measure; a stopped measurement
  leaves a gap in a graph; a PID is paired with its start time so an action cannot hit
  a recycled PID; no telemetry leaves the machine.

## 2. The window (shared chrome)

Reference: every `reference/app/*.png`.

```
┌────────────┬──────────────────────────────────────────────────────────────┐
│ ● ● ●      │ [▯] Page Title (title font, ~32 pt)        [page actions]    │
│ Summary    ├──────────────────────────────────────────────────────────────┤
│ Performance│ ⓘ notice banner (optional, dismissible)        [Yes][No] ✕  │
│ Processes  ├──────────────────────────────────────────────────────────────┤
│ System Info│                                                              │
│ Startup    │                     page content                             │
│ Users      │                                                              │
│ Services   │                                                              │
│ ── MORE ── │                                                              │
│ Power&Freq │                                                              │
│ Processor  │                                                              │
│ Recorder   │                                                              │
│ Connections│                                                              │
│ Installed  │                                                              │
│ Drivers    │                                                              │
│ Disk Space │                                                              │
│ Benchmarks │                                                              │
│            │                                                              │
│  ~logo~    │                                                              │
├────────────┼──────────────────────────────────────────────────────────────┤
│⚙ Settings │ ☀/☾ theme │ ● Providers healthy · 542 processes │ IP … │ Gen 81 │ ● ■ ● │
└────────────┴──────────────────────────────────────────────────────────────┘
```

- **Sidebar** (~240 pt wide, agg-gui's panel colour for the current theme): one
  row per destination on a ~36.5 pt pitch, Font Awesome icon + label (~14 pt in Noto Sans; the
  reference's cap height is ~10 pt), rounded ~34 pt selection pill in the theme's selection colour. TMOG splits the list with a small "PRO" divider; ours keeps the
  divider as a plain group separator labelled **MORE** (we have no Pro).
  The logo sits near the bottom; below it, a footer row with **Settings** and
  a **theme** button (sun/moon) that switches agg-gui Light / Dark. TMOG's Colors
  popover is not reproduced.
- **Title bar row** (~53 pt tall, a separator line under it): sidebar toggle icon, then the page title, large, in agg-gui's default font. Page actions on the right: a filter field ("Filter by name, user, or PID"),
  a view combo ("Process View"), Reload, Export…, Reset, Start, depending on page.
- **Notice banner**: pale blue strip with an info icon, message, and optional
  Yes / No / ✕ — used for one-time questions (e.g. "Open TaskManager with
  Control-Shift-Escape?" on Windows/macOS/Linux where we can register it). Built on an
  agg-gui banner widget (add one to agg-gui if missing).
- **Status bar** (~36 pt tall, level with the sidebar footer): green dot + "Native providers healthy" (or which provider is degraded),
  process count, primary IP, sample **Generation N** (monotonic sample counter), and on
  the right the Flight Recorder transport: record (red LED), stop, live indicator and a
  VFD time readout `00:00:00.00` while recording or replaying.
- Window: native title bar (traffic lights on macOS). Minimum 1100 x 700 pt; default
  1242 x 791 pt (the reference window). Window size and position remembered.

## 3. Instrument widgets (custom, in `taskmanager-core/src/ui/instruments/`)

These carry the look and get the most care. Each is a reusable agg-gui widget with a
headless test and a screenshot example.

| Widget | Look (references) | Used by |
|---|---|---|
| **SegmentBar** | Row (or column) of rounded LED cells; lit cells in the channel colour with a soft glow ("bloom"), unlit cells dim (dark grey in Light, channel colour at ~15 % in Dark). Optional colour ramp across the bar (green → yellow → red for CPU on Performance). Value text to the right in the channel colour (`32.8%`). | Summary tiles, Performance headers, Summary column meters (vertical), Flight Recorder, Disk Space scan bar, Benchmarks |
| **ColumnMeters** | 4 vertical SegmentBars under small coloured labels (CPU, Power/Clock, Temp, GPU) with readings below. Optional VFD header (`SYS 14.8W`). | Summary |
| **TraceGraph** | Scrolling time series on a fine grid (major + minor lines), border in the channel colour, line in the channel colour with glow, optional filled area, left-pointing ▸ marker at the newest sample on the right edge. Several series per graph (utilisation green, kernel red, temperature orange) with independent axes (left %, right °). Gaps where samples are missing. Optional "compress older history" x-scale (1x/2x/4x/8x regions). Min/max labels in a corner (`20.5 GB` / `18.5 GB`). | Everywhere |
| **MiniGraph** | Small TraceGraph without labels, rounded border, for list thumbnails (Performance device list, per-core grid cells with a `CPU 4` caption). | Performance, Processor page |
| **VfdText** | Vacuum-fluorescent dot-matrix / segment text: cyan or blue glowing glyphs on a dark navy panel with unlit segments faintly visible (`CPU OVERVIEW ……… 35%`, `TOP 14 CPU PROCESSES`). Our own glyph table, drawn as vector cells. | Summary card headers, Benchmarks readouts, status-bar clock |
| **MetricTile** | Card: icon + title (large) + headline value in channel colour, subtitle line, SegmentBar along the bottom. Pale channel-tinted background in Light. | Summary bottom row |
| **StatGrid** | Label (small grey) over value (bold) cells in 4 columns, and a two-column "key: value" spec table. | Performance detail areas |
| **Gauge** | Round analogue dial: chrome bezel, dark carbon face, white ticks and numerals, red needle with hub, embedded VfdText readout and unit label (`Mops/sec`). Animated needle. | Benchmarks |
| **BubbleMap** | Circle-packing of files and folders by size, coloured by file type (media red, documents green, code blue, archives yellow, apps purple, system grey), on a dark panel. | Disk Space |
| **UsageCell** | Table cell whose background is a horizontal fill proportional to the value, tinted green for CPU and memory, orange for high memory (Users), red rows for exited processes. | Processes, Users |

Every instrument reads its colours from the theme's **ChannelPalette** (Light/Dark
values per channel: cpu, kernel, memory, gpu, npu, disk, network, energy, thermals) and
follows the agg-gui theme (Light or Dark). No saturation, bloom, mono or VFD
settings: the glow and VFD look are fixed.

Fonts: agg-gui's default face (Noto Sans) everywhere, titles set large; no bundled title font;
Font Awesome 4.7 icons by code point (`fonts::fa`); VfdText draws its own glyphs.

## 4. Telemetry architecture

### 4.1 Readings never lie

```rust
pub enum Reading<T> {
    Value(T),          // the provider measured this
    Unavailable,       // supported here, but no sample this tick (permission, sensor busy)
    Unsupported,       // this platform / machine cannot measure it
}
```

UI rules: `Value(0)` shows a real zero (`0 B/sec`, `0.0%`), as the
app captures do; `Unavailable` shows "Unavailable"; `Unsupported` shows "—" or hides the
row; graphs break the line across non-`Value` samples.

### 4.2 Samples and history

- One **Sampler** owns the providers and produces a `Snapshot` per tick (default 1 s,
  settable 0.5–5 s): system totals, per-CPU, memory composition, per-disk, per-NIC,
  GPU/NPU, power, thermals and the process table. Each snapshot gets the next
  **Generation** number.
- **History** keeps a fixed-capacity ring per series (`Reading<f32>`, 3600 samples by
  default), plus the compressed-history view (older samples averaged into 2x/4x/8x
  buckets aligned to the absolute sample count; a bucket with no values stays a gap).
  `HistoryStore` maps `SeriesId` → `History` and pushes into every series each tick
  (`Unavailable` for a device that vanished, back-fill for one that appeared).
- Static facts (CPU, memory, disks, NICs, GPU/NPU) are a separate `MachineInfo`, read
  when the sampler starts and on refresh.
- Production and consumption are split: `SampleProducer` (owns the provider; drift-free
  schedule, due time += interval, resync only when a whole interval behind; stamps
  generation and health; keeps vanished processes as `Exited` rows for 3 s; when a
  listing fails, `processes: Reading<ProcessTable>` comes back non-`Value` and the last
  table is carried forward with live values `Unavailable`, never as a mass exit) emits
  `Arc<Snapshot>`; `TelemetryState` (latest snapshot, `HistoryStore`,
  `Arc<MachineInfo>`, health) ingests them. `Sampler` composes both on one thread.
- Natively the `SampleProducer` runs on a worker thread and hands `Arc<Snapshot>`s to
  the UI's `TelemetryState` through agg-gui's `ui_thread` drain; on wasm the demo
  `Sampler` is stepped from the frame clock.

### 4.3 Providers: the platform seam

`trait TelemetryProvider: Send` (core: `machine_info()`, `sample(now) -> Snapshot` with
every field a `Reading` and rates computed by the provider, `health()`), and
`trait PlatformServices` (core) for actions and inventories (M1 has `is_demo`,
`end_process`, `open_url`; the rest arrive with their milestones). `PlatformServices` is
`Send + Sync`; a platform's provider and services share state through `Arc<Mutex<…>>`
(so ending a demo process removes it from the next demo snapshot):

- actions: end task / force quit (identity-checked, see 4.4), reveal in file manager,
  open URL, enable/disable startup item, start/stop service, uninstall app (to Trash);
- inventories: system info categories, startup items, users, services, installed apps,
  drivers, connections, sensors, volumes;
- `is_demo()` so widgets never `cfg` on the target.

Implementations:
- Demo platform (core, all targets, `platform/demo/`): `demo_platform(seed) ->
  (DemoProvider, DemoServices)` over one shared `Arc<Mutex<DemoMachine>>`; a
  deterministic simulated machine with plausible load curves, process churn (processes
  start and exit), ~540 processes, and every inventory filled. Seeded, and its clock is
  the time since the first sample, so tests and screenshots are reproducible. Ending a
  root-owned demo process returns `PermissionDenied` (the demo user may not signal it);
  ending any other process also removes its descendants (an app's helpers quit with it).
- `NativePlatform` (taskmanager-native): `sysinfo` for the cross-platform base (CPU,
  memory, processes, disks, networks, users, components/temperatures), plus per-OS
  modules for what sysinfo lacks:
  - **macOS**: `host_processor_info` per-CPU kernel/user split, `vm_statistics64`
    (wired, compressed, pressure), IOKit for GPU utilisation, battery/AC and SMC power
    where readable, IOReport for P/E cluster and media-engine residency where readable
    without root, `launchctl`/plists for startup items and services, `kextstat`-style
    listing via `KextManager` for drivers, `proc_pidinfo` + `proc_pidfdinfo` for sockets,
    `/Applications` bundles for installed apps, `system_profiler -json` for System Info.
  - **Windows**: PDH counters (per-CPU, kernel time, disk active time), DXGI/D3DKMT for
    GPU engines, `GetSystemPowerStatus`, Services via the SCM, startup items from Run keys
    and the Startup folder, installed apps from Uninstall keys, drivers via
    `EnumDeviceDrivers`/SetupAPI, TCP/UDP tables via `GetExtendedTcpTable`, WMI for
    System Info.
  - **Linux**: `/proc` and `/sys` (stat, meminfo, PSI pressure, diskstats, net/dev,
    hwmon, powercap RAPL, cpufreq), systemd over D-Bus (`busctl`/zbus) for services,
    XDG autostart for startup items, `/proc/net/{tcp,udp}` for sockets, `lsmod` data from
    `/proc/modules` for drivers, `.desktop` files + dpkg/rpm/flatpak for installed apps,
    DMI from `/sys/class/dmi` for System Info.
- Pure parsing for every one of these formats lives in the core (`platform/parse/…`) so
  it is unit-tested on every OS from fixture text.

### 4.4 Process identity

`ProcessKey { pid, start_time }`, where `start_time` is the OS's fine-grained creation
stamp, monotonic within a boot (Linux `/proc/<pid>/stat` starttime ticks; macOS
`proc_pidinfo` start `tv_sec`+`tv_usec`; Windows `GetProcessTimes` creation FILETIME).
sysinfo's one-second wall-clock `start_time` is not enough: a pid reused within the
same second would keep its key. Every action takes a `ProcessKey`, re-reads the target's
start time immediately before acting, and refuses (with a toast) if it changed. Selection
in tables is by `ProcessKey` too, so a recycled PID never inherits a selection.

## 5. Screens

Each section names its reference capture. "Card" = rounded panel with a 1 pt border on
the window background.

### 5.1 Summary — `app/01-summary.png`
Top row, three cards:
1. **Column meters**: VFD header `SYS 14.8W`; four vertical SegmentBars CPU (green),
   Power (red), Temp (yellow/amber), GPU (blue); readings below (`35.1%  14.8 W
   57.6 °C  26.0%`).
2. **CPU Overview**: VFD header `CPU OVERVIEW … 35%`; legend Utilization (green),
   Temperature (orange), Kernel (red); TraceGraph with 0–100 % left axis and 0–110°
   right axis; footer "10 logical processors · 35.1% · 14.8 W" and "Speed Auto".
3. **Top N CPU processes**: VFD header `TOP 14 CPU PROCESSES`; table PID, app icon +
   Name, CPU (green), Memory; alternating row tint; N fits the card height.

Middle: **Memory Utilization** card spanning the width: vertical magenta SegmentBar with
`83.8%` under it, TraceGraph with min/max labels, title and `20.1 GB / 24.0 GB`, footer
Available / Cached / Swap.

Bottom: four MetricTiles — Network (blue, rate, link speed and utilisation), the primary
disk by model name (green, %, R/W, SMART health), NPU (red/amber), and **System
Pressure** (CPU queue, memory pressure, I/O, headline NORMAL/ELEVATED/CRITICAL with a
green→yellow→red bar). The older layout (website) shows six tiles in two rows (Disks,
Network, Energy, GPU, NPU, Thermals); the tile set adapts to window height.

### 5.2 Performance — `app/02-performance-*.png`
Left: device list (~470 px at 2x), one row per device: icon, large name, two grey
subtitle lines, MiniGraph thumbnail on the right; selected row is a solid accent pill
with white text. Devices: CPU, Memory, GPU n, NPU n, each physical disk (or "Disks"
combined), Network (combined, or per interface), Energy, Thermals.

Right: detail page for the selected device. Common header: large device name (~48 pt),
a long SegmentBar with the headline value at the right in the channel colour, a subtitle
line, then the graph area and a StatGrid.
- **CPU**: "% Utilization by processor class" with a *Core classes* menu (Overall /
  Core classes / Logical processors / NUMA): Super/Performance and Efficiency cores
  side by side, or a grid of per-logical-CPU MiniGraphs. StatGrid: Utilization, Speed,
  Processes, Threads, Up time; spec table: base speed, cores, sockets, logical
  processors, virtualization, VM, L1/L2/L3, frequency driver/governor, power preference.
- **Memory**: big magenta TraceGraph; StatGrid: In use, Available, Committed, Cached,
  Swap used/available/total, Speed, Slots used, Form factor, Type, Compressed, Wired,
  App memory, Pressure, Pressure load, Page ins/outs.
- **GPU**: utilisation graph(s) per engine, memory, model and cores.
- **NPU**: active residency / blocks powered.
- **Disk**: "% Active time" graph and "Disk transfer rate" graph; read/write speed,
  totals, active time, response time, capacity, formatted, system disk, type; "Storage
  details…" button.
- **Network**: throughput graph with adaptive scale; receive/send, totals, interface,
  connection type, hardware address, IPv4/IPv6; "Connection details…" (opens
  Connections).
- **Energy**: power consumption graph; three status chips (Thermal state, Power mode,
  Power source); "Processes with the highest estimated energy demand" as name + bar +
  score rows, with a footnote on how the score is estimated.
- **Thermals**: hotspot graph with °C scale; chips Thermal pressure and Hotspot; a
  2-column grid of "Temperature sensors" cards, each a MiniGraph with name and reading.

### 5.3 Processes — `app/03-processes.png`
Header actions: **Process View** combo (Process View / Flat / By user) and a filter
field. Table columns Name, PID, Status, User name, CPU usage, Memory usage (sortable;
active sort column bold with chevron). Grouped into **Applications (n)** and
**Background processes (n)**; expandable children (disclosure chevrons), app icons,
UsageCell fills for CPU/memory, exited processes kept briefly in red, "Access denied"
where a value is not readable. Rows are rounded pills on a light background.
Bottom **details pane** for the selection: name (PID), executable path, and five
columns Identity (parent, user, status, architecture), Lifetime (started, uptime,
threads, priority), Processor (CPU, CPU time, GPU, NPU), Memory (footprint, private,
peak, page faults), Disk (reading, writing), Network (throughput).
Context menu: End task, Force quit, Reveal executable, Copy path, Properties. Delete key
= End task with confirmation.

### 5.4 System Info — `app/04-system-info.png`
Left: category list with icons (Hardware Overview, Memory, Audio, Bluetooth, Camera,
Card Reader, Graphics/Displays, Controller, NVMExpress, SPI, Secure Element, Smart
Cards, Thunderbolt/USB4, USB, Power — per-OS set). Right: icon + category title,
**Export…** menu (text, JSON), and a two-column key/value table with row separators.
**Reload** in the title row.

### 5.5 Startup apps — `app/05-startup-apps.png`
Filter field. Table Enabled (checkbox), Name, Command; empty pill rows fill the rest;
bottom details area ("Select a single entry for detail information." when none).
Toggling a checkbox enables/disables the item (with confirmation for system items).

### 5.6 Users — `app/06-users.png`
Filter field. Table User (expandable to the user's processes), Status, Processes, CPU,
Memory (UsageCell green/orange), PID. Details area as 5.5.

### 5.7 Services — `app/07-services.png`
Filter field. Table Running (checkbox), Name, Description (executable), Group (plist /
unit / service group). Details area; Start/Stop/Restart in the context menu.

### 5.8 Power & Freq — `app/08-power-freq.png`
Sensor tree (TreeView with columns Sensor, Value, Min, Max): machine → CPU → groups
Temperatures / Powers / Utilization / Clocks → sensors; then GPU, disks, battery. Group
icons coloured per kind (thermometer orange, bolt yellow, gauge green, clock red). Min
and Max are tracked since launch (Reset in the title row).

### 5.9 Processor (TMOG: "Apple Silicon") — `app/09-apple-silicon.png`
Dark panel page. Chip name, large, (`Apple M5`), subtitle with core counts. One card
per core class (Performance / Efficiency, or a single class on x86) with % headline, a
class TraceGraph and a grid of per-core MiniGraphs; a **Media Engine** card with
residency graphs per block (Video encode/decode, ProRes) on Apple silicon, or GPU
engines (3D, Copy, Video) on Windows. Title "Apple Silicon" on Apple silicon Macs,
"Processor" elsewhere.

### 5.10 Flight Recorder — `app/10-flight-recorder.png`
Machine name and core summary; a transport strip (● LIVE, **Record**, **Replay…**); six
panel cards in a 2x3 grid: Performance cores, GPU cores, Neural Engine, Media Engine,
Thermals, Memory — each a TraceGraph with a small per-unit SegmentBar strip. Recording
writes our own `.tmtrace` format (versioned, chunked, zstd-compressed snapshots);
replay scrubs with the status-bar transport and drives every page from the trace
instead of the live sampler.

### 5.11 Connections — `app/11-connections.png`
Five stat cards (Connections, Listening, Public, Processes, System traffic with a two-line
mini chart). Segmented filter (All / Connected / Listening / Public / UDP / Local IPC),
"n shown of m", visibility note, refresh. Table Application (icon by state, name, PID ·
user), Protocol (TCP/UDP + IPv4/6), State (LISTEN in green, monospace), Local endpoint
(monospace), Remote endpoint. Right detail panel: connection, endpoints, socket.

### 5.12 Installed Apps — `app/12-installed-apps.png`
Left: sortable list Name / Size with checkboxes and icons, footer "All Apps ˅ ·
n applications". Right: big icon, name + version, Editor/Opened/Modified, **Uninstall…**
(moves to Trash; on Windows runs the registered uninstaller), Related Files (n) list of
support files outside the bundle.

### 5.13 Drivers — `app/13-drivers.png`
Dark panel page. Four stat cards (Drivers, Loaded, Third-party, Wired memory); segmented
filter (All / Loaded / Not loaded / Third-party / Extensions), "Updated <time>",
refresh. Table Driver (name + bundle id), State, Version, Wired, Vendor; selected row is
a solid accent pill. Right detail panel with key/value list, an info note (e.g. "System
Integrity Protection does not permit unloading…"), **Reveal in Finder**, **Copy
Details**.

### 5.14 Disk Space — `app/14-disk-space.png`
Toolbar: volume menu, Group: File Size, View: Balls/Treemap, path; **Start** in the title
row. Left: BubbleMap on a dark square; scan SegmentBar with status ("Sample scan ·
Complete"); collapsible **Largest Files (100 tracked)**. Right: volume card (name,
capacity, used/free bar), **File Type** legend with sizes, **Largest Folders**,
**History** of scans. The scanner is a small parallel walker (jwalk) shared in spirit
with HDTreeMap but implemented here.

### 5.15 Benchmarks — `app/15-benchmarks.png`
Left list: CPU, GPU, Disk, Internet, **TaskManager Score**. Right: large title
("CPU Benchmark"), two Gauges (Single core Mops/sec, Multi core Gop/sec), a progress
SegmentBar, **Start**, status ("Ready"), **History** with Clear History…; **Reset** in
the title row. Our own benchmark kernels (integer/float mix, memory bandwidth, disk
sequential/random, an HTTP download test against a URL the user configures).

### 5.16 Settings — `app/16-settings-*.png`
Settings is a page (optionally a floating window) of grouped cards: **Appearance**
(Language, Temperature unit, Display font, Live dock/taskbar icon, menu bar/tray item
and its contents, High-frequency visuals 30/60/120 fps, Show Settings as a dialog);
**Graphs** (Color keyed graphs, Dashed lines on summary graph, Smooth graph curves,
Compress older history, History multiplier, Pixels per update); **Processes**
(include TaskManager itself, per-core CPU statistics); **Updates**; **Global shortcut**
(Open with Control-Shift-Escape); **Window management** (Always on top, Hide on close).
TMOG's popout, skins, screensaver and remote-connection settings are out of scope.
Theme is one setting, App theme (Match System / Light / Dark), mapped straight onto
agg-gui's Light and Dark visuals; TMOG's Colors popover (display mode, VFD text,
saturation, bloom, HDR) is out of scope.

## 6. Crate layout (mirrors HDTreeMap)

```
taskmanager-core/     rlib, everything testable, no platform code
  src/fonts.rs          font sizes + Font Awesome code points (fa::*)
  src/theme.rs          ThemeChoice {System, Light, Dark}, agg-gui Visuals, ChannelPalette
  src/format.rs         bytes, rates, %, °C/°F, durations; sizes one decimal ("2.2 GB"),
                        rates three significant figures with "/sec" ("35.8 KB/sec")
  src/reading.rs        Reading<T>
  src/telemetry/        snapshot (Snapshot), machine (MachineInfo), history (ring +
                        compression), series (SeriesId, HistoryStore), producer
                        (SampleProducer), state (TelemetryState), sampler (both)
  src/process/          key (ProcessKey), table (rows, grouping, exited retention), tree,
                        kind (Application / Background heuristics)
  src/platform/mod.rs   TelemetryProvider + PlatformServices traits
  src/platform/demo/    demo platform: simulated machine, demo_platform(seed) (all targets)
  src/platform/parse/   pure parsers: proc_stat (/proc/<pid>/stat), procfs (stat, meminfo,
                        PSI, loadavg), sysfs (caches, link speed), desktop (.desktop),
                        net (interface kinds, networksetup), logical_processor (Windows
                        cache records); later launchctl, netstat tables, …
  src/recorder/         .tmtrace writer/reader, replay clock
  src/settings.rs       persisted settings (JSON) + SettingsStore trait
  src/state/            AppState: current page, theme state (choice, OS appearance,
                        installed look), selection, sampler pump, notices
  src/ui/               mod.rs root (sidebar + page host + status bar), chrome/,
                        instruments/ (§3), pages/<one module per screen>
  tests/                headless UI tests per page, file_line_count, parsers
  examples/             screenshot_pages (render every page to PNG from the demo),
                        compare_refs (side-by-side with reference/ for review)
taskmanager-native/   bin `taskmanager`: agg-gui-shell + NativePlatform (platform/: mod.rs
                        provider + services, processes, devices, machine, rates, sysinfo
                        base + {macos,windows,linux,unix}.rs), CLI (cli.rs, dump.rs for
                        --dump-snapshot), persistence, menu bar
taskmanager-wasm/     cdylib: agg-gui-web-shell + DemoPlatform
demo/                 static page + bun/vite bundle for GitHub Pages
```

## 7. Testing and visual verification

- `cargo test --workspace`: unit tests for formatting, Reading/History (gaps,
  compression), process identity, grouping, every parser from fixtures, recorder
  round-trip; headless UI tests that build each page on `DemoPlatform`, drive input
  through agg-gui's test harness and assert on widget state.
- `file_line_count` test: every first-party text file ≤ 800 lines.
- **Screenshot harness**: `TASKMANAGER_SCREENSHOT=out.png TASKMANAGER_PAGE=summary
  cargo run -p taskmanager-native -- --demo` renders one frame and exits;
  `cargo run -p taskmanager-core --example screenshot_pages -- out/` renders every page
  (and every Performance device) headless at 2484 x 1582 from the seeded demo machine,
  Light and Dark.
- **Reference comparison**: `cargo run -p taskmanager-core --example compare_refs` writes
  `target/compare/<page>.png` (ours | reference, side by side) plus a coarse layout score
  (mean difference over a 32 x 20 grid of luminance and hue). Used by the implementer
  and reviewer at the end of each page step and in PR descriptions; it is not a CI gate
  because `reference/` is not in the repo.
- CI (Linux, macOS, Windows): fmt, clippy `-D warnings`, tests; wasm32 check.

## 8. Workflow

Orchestrator (main Claude session) splits each milestone into small steps; the
**implementer** subagent builds one step; the **reviewer** subagent reviews it against
this plan, CLAUDE.md and the reference image; the orchestrator fixes or re-delegates,
then commits. One commit per green step. `scripts/pre-commit-check.sh` before every
commit.

## 9. Milestones

| | Milestone | Done when |
|---|---|---|
| M0 | Scaffold | Workspace, CLAUDE.md, CI, pre-commit, launchers, file_line_count; window opens with sidebar, title row, empty pages, status bar; Light/Dark theme; screenshot env var works. |
| M1 | Telemetry core | Reading, Snapshot, Sampler, History with gaps + compression, ProcessKey; DemoPlatform; NativePlatform base on sysinfo for CPU, memory, processes, disks, network on all three OSes. |
| M2 | Instruments | SegmentBar, ColumnMeters, TraceGraph, MiniGraph, VfdText, MetricTile, StatGrid, UsageCell, with screenshot examples matching the references' look. |
| M3 | Summary | §5.1 matches `01-summary.png` in Light and the website's dark summary. |
| M4 | Performance | All devices of §5.2; per-OS extras for kernel time, memory composition, GPU, power, thermals. |
| M5 | Processes | §5.3 incl. grouping, tree, details pane, identity-safe End task / Force quit. |
| M6 | Inventories | System Info, Startup apps, Users, Services (§5.4–5.7) on all three OSes. |
| M7 | Hardware | Power & Freq, Processor (§5.8–5.9). |
| M8 | Network & drivers | Connections, Drivers (§5.11, 5.13). |
| M9 | Apps & disk | Installed Apps, Disk Space (§5.12, 5.14). |
| M10 | Benchmarks | Gauge widget + CPU/GPU/Disk/Internet kernels + score (§5.15). |
| M11 | Recorder | .tmtrace record/replay driving all pages (§5.10). |
| M12 | Settings & polish | Settings page (§5.16), global shortcut, tray/menu bar item, persistence, About. |
| M13 | Web demo & release | wasm demo on GitHub Pages; release workflow (macOS universal .app, Windows zip, Linux tar.gz/AppImage); README with hero. |

## 10. Open questions

- Which privileged metrics are reachable without root/admin on each OS (macOS IOReport
  power and residency, Windows per-process GPU, Linux RAPL needs read access to
  `/sys/class/powercap`); unreachable ones are `Unavailable`, never faked.
- Global Control-Shift-Escape: on Windows it belongs to the system Task Manager; we offer
  it only where we can register it without replacing system behaviour.
