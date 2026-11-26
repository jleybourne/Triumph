# Desktop launcher: design

Status: design only, nothing built. Grounded in CLAUDE.md, run-title.ps1, the README "Running it", design/game-saves.md and design/dlss.md. Profiles (branch `profiles`) are assumed, not read.

## 1. Technology

Choice: a Rust-native egui (eframe) app, new crate `apps/victory-launcher` in the workspace, one binary beside `victory.exe`.

- Same language, same toolchain pin, GPL-3.0 compatible (egui, eframe, serde, toml, sysinfo/windows crates are MIT/Apache). No runtime, no installer, no webview.
- It can link the workspace crates directly for the loader pieces that must agree with the layer: the exe build-id computation, the victory.toml schema, the profile schema. A web or Tauri shell would need a second implementation or a subprocess for each.
- egui is immediate-mode: easy for tables, progress bars and live logs, which is most of this UI. It is weak at accessibility and native look; acceptable for a tester tool. iced is retained-mode and nicer for forms but heavier to learn and slower to iterate. Pick egui; keep all logic in a UI-free `launcher-core` module so the shell can be swapped later.
- Rejected: Tauri/Electron (a browser engine and npm for a tool this small, network surface), C#/WPF (second language, second licence story), PowerShell GUI (what we are escaping).
- Constraints met by construction: no telemetry, no sockets (no network crate in the dependency tree; cargo-deny rule enforces it), no bundled game or third-party binaries. DLSS DLL is only ever copied from a path the user picks.

## 2. Screens (ASCII)

```
First run: Setup
+------------------------------------------------------+
| Game folder  [ ............................ ] [Browse]|
| Files        [ok] main exe  [ok] data pack  [!!] x    |
| Build id     0x........ (as the loader computes)  [ok]|
| Requirements                                          |
|  GPU: <name>              [ok]   Driver: <ver> [ok]   |
|  Shader model 6.6         [ok]   Mesh shaders  [ok]   |
|  Windows commit  X GB free [ok]  Disk  Y GB free [ok] |
|  Missing: (list, each with a one-line fix)            |
|                              [Re-check] [Continue >]  |
+------------------------------------------------------+

Main (tabs: Play | Profile | Switches | Cache | Saves | Logs)
+------------------------------------------------------+
| Play                                                  |
| Profile [ Series X v ]  Build [ diag v ]              |
| Resolution [ 2560x1440 v ]  Window [ Borderless v ]   |
| Scale [ FSR 3 v ] [ DLSS (needs DLL: set path) ]      |
| Look   [ release-look preset v ]                      |
| [ Launch ]            Status: not running             |
|  fps 7.6  ms 131  draws 4,500  dropped 344  (live)    |
|  [Stop]  [Summarise run -> frames.py]                 |
+------------------------------------------------------+

Switches (testers)
| [x] Confirmed set (locked list, shown read-only)      |
| Experiments (off by default, warning banner)          |
|  [ ] LIGHT_REFRESH   unfinished; crashed in all-on set|
|  [ ] PRECOMPILED     measured worse (0.115 ms/draw)   |
|  Each: name, description, last measured, run row ref. |
| Effective environment preview  [Copy as PowerShell]   |

Cache
| Shader cache  [#######...] 612/1433  [Warm] [Cancel]  |
| Pipeline library: present, N entries, size, age       |

Saves
| Backups: <timestamp> <size>  [Backup now] [Restore..] |
| Restore asks for confirmation; auto-backup before run.|

Logs
| Run list | text viewer w/ filter (warn/error) | crash  |
| [Open folder] [Copy for bug report (paths redacted)]  |
```

## 3. Architecture

- `launcher-core` (no UI): config, validation, requirements, profile apply, process control, log tail, summaries. Unit-testable without a GPU or game.
- Config: reads and writes the same `victory.toml` (game and data paths); launcher adds a small `launcher.toml` in the user's config dir (last profile, DLSS DLL path, backup location). Paths stay local; never shown in shared output.
- Validation: file list and build id come from a library function or `victory.exe config` / a new `victory validate --json` (preferred, one source of truth). Requirements: DXGI adapter enumeration, `CheckFeatureSupport` (shader model 6.6, mesh shader tier), `GlobalMemoryStatusEx` for commit, `GetDiskFreeSpaceEx`. Same checks reused as a `victory doctor` subcommand so the CLI users benefit.
- Launching: spawn `victory.exe run <exe> --execute` as a child with a constructed environment block (never mutating the launcher's own env). Confirmed set is a data table (`switches.toml`: name, value, status confirmed/experimental, description, evidence row), single source shared with the scripts. Profile supplies `--profile <name>`; the launcher does not re-implement profile semantics, it only lists profiles (`victory profile list --json`) and passes the name. Title settings backup/restore is the layer's job (profiles); the launcher shows its state and calls it.
- One-instance guard: before launch, check for a live victory process and refuse (stale-log false conclusions in CLAUDE.md); after launch confirm a new log timestamp.
- Live fps: tail the run log or the frame-account line (`usual frame`); better, a `--stats-pipe` / JSON-lines file written by the layer. Parse tolerant, show fps, ms, draws, dropped together (a win with fewer draws is not a win).
- Measure: run `frames.py` as a subprocess if Python is found, else a built-in Rust port of the summary; show the table and save it beside the log.
- Warm-up: spawn `victory translate --warm` and parse progress lines (ask for `--progress json`).
- Saves: file copy of the saves folder (the layer's own VICTORY_SAVE_BACKUP idea in game-saves.md); restore only when no victory process runs, move current aside first, never delete.
- Safety: no network, no auto-update; Windows only; avoid compiling during a run (not an issue, the launcher never builds).

## 4. Effort

- Minimal v1 (about 1.5 to 2 weeks): setup validation, requirements, profile picker (names only), confirmed-set launch with a one-instance guard, log tail with fps, open logs folder. Egui single window, no warm-up, no saves.
- Full (about 5 to 6 weeks): experiments panel with evidence, warm-up with progress and pipeline status, saves backup/restore, crash viewer, frames.py summaries and run history compare, DLSS and release-look pickers, redacted bug-report bundle, installer-free zip.

## 5. Stages

1. `launcher-core` plus `victory validate --json`, `victory doctor`, `victory profile list --json`; CLI-only, tests.
2. v1 UI: Setup and Play tabs.
3. Switches tab backed by `switches.toml`; replace the env blocks in the scripts with the same file.
4. Logs tab, live stats (needs stats output from the layer).
5. Cache tab (needs `--warm` progress output).
6. Saves tab.
7. DLSS and look presets once those exist; run history and summaries.

## 6. Dependencies on in-flight work

- Profiles: the picker, display modes, upscaler choice and title settings backup/restore all depend on the profile schema and `--profile`. Stage 2 can ship with a stub list until then.
- DLSS plugin: the DLSS option is hidden unless the plugin and a user DLL path exist; the plugin's discovery rule (path, version check) must be defined first. Licensing of the SDK stays the user's step.
- Saves: backup is a plain copy today; versioned containers with `.bad` quarantine and the optional layer-side backup (game-saves.md) would change what restore must handle.
- Layer changes asked for: machine-readable progress and stats output, a single switch table, a validate command. Small, but they are what keeps the launcher thin.
- Caveat: the debug build is the only measurable one (release reaches 13 draws); the launcher must default to the diagnostics build for measure runs and say so.
