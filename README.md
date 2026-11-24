# Triumph

A compatibility layer that runs a Series-generation console title natively on
Windows, from files you supply. The title's own x86-64 code runs unchanged
inside our process; Triumph provides the platform underneath it and converts
its data — ahead of time where it can, on first use where it must (shaders).

Nothing here contains, decrypts or distributes game data.

> The binary and crates are still named `victory` (inherited from the upstream
> project this was derived from). Only the repository is called Triumph.

## Status

**Playable: no. Loads into the world: yes. Renders the world: not yet.**

The runtime boots the title, plays the intro, reaches the world and accepts
input — the player moves, the camera turns, the pause menu opens. The screen
stays black.

That is not a renderer fault. The title is GPU-driven: its compute passes build
the frame. A world frame reports:

```
draws=4218  drawn=4218        every draw executes
dispatches=512 dispatched=0   no compute pass executes
```

All 512 dispatches are dropped because their shaders do not translate yet. With
no culling, visibility or setup passes the draws have nothing to draw, the depth
buffer stays flat, and the presented frame is black.

## What works

| Area | State |
|---|---|
| Boot, first-run screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video and loading art | works, full colour |
| Start screen, profile, saves | works |
| Loading into the world (~50 s) | works |
| Input: movement, camera, pause menu | works |
| Renderer | a fully lit 3D image is produced for the cinematic stages — verified from captured render targets, so translation, presentation and texture handling work there |
| Device removals | fixed: render-target views, pipeline states and clear values agree on one fully qualified format |
| Console wave size | declared only to programs that read lanes, so 32-lane hardware can create those pipelines at all |
| Deliberate aborts | the title's own crash-forcing write (`mov [0], tag`) and the `int3` after it are stepped over and logged instead of ending the process |
| Framerate | 30 fps in menus, 1–3 fps in the world |

## What does not work

**1. The world is black.** 1,007 of the compute shaders the title uses fail to
translate; with them go every compute pass. Failure classes in the compute set:

| count | cause |
|---|---|
| 274 | lane mask read as data |
| 115 | descriptor read only as data |
| 87 | select on an untracked mask |
| 85 | lane mask holding lanes elsewhere, read as data |
| 34 | address in an SGPR that is no root parameter |
| 24 | cross-lane operation outside the entry block |
| 17 | address not from a root parameter |

The mask family (446) and the address family (166) are the two clusters worth
attacking; both are design changes in the translator, not missing opcodes.

**2. Speed.** World frames take 600–900 ms against the title's own budget. The
translated code costs roughly two orders of magnitude more per lane than the
console's, so this is compiler efficiency (per-lane instruction count, 16-bit
paths, wave64), not scheduling.

**3. Smaller gaps.** Render-target metadata planes; reverb; HDR and dynamic
resolution; the ray-tracing builder's shaders.

## Translator census

`victory translate` over the title's 26,084 programs:

```
vertex and pixel  12,766 of 14,027
compute            1,464 of  2,582
mesh               8,215 of  8,713
```

## Where it runs

Development and testing are on one machine:

| | |
|---|---|
| GPU | NVIDIA RTX 4070 Ti, 12 GB, driver 610.88 — 32-lane waves, no 64-lane support |
| CPU | Intel i7-10700KF — no CLWB/CLZERO; the runtime emulates the console's AMD-only instructions |
| Memory | 64 GB |
| OS | Windows 11, build 26300 |
| Display | 2560×1440 (the game window is placed 16:9 on the right) |
| Toolchain | Rust 1.98.1, MSVC 14.51, DXC 10.0.26100, Windows SDK 10.0.26100 |

Windows only. Linux and macOS are not targets.

## Building and running

Requirements: Windows 10/11 x64, AVX2, a Direct3D 12 GPU with shader model 6.6
and mesh shaders, Rust (pinned in `rust-toolchain.toml`). The title needs several
GiB of Windows commit at startup — check **Committed** in Task Manager if
`crashes.log` reports error 1455.

```powershell
copy victory.example.toml victory.toml     # then set your paths (outside the repo)
cargo test --workspace                     # needs no game data
cargo run -p victory -- config             # check your paths

cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log` live
under the data folder (default `%LOCALAPPDATA%\victory\data`); derived data
under the cache folder.

Input: XInput pads. With none connected the keyboard acts as a pad while the
title's window has focus:

| Keys | Pad |
|---|---|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

## Commands

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
```

## Diagnostics

A release build carries none of these; build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|---|---|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_KEYS=30:Enter,35:X` | press keys at those seconds, focus or not |
| `VICTORY_CAPTURE=<dir>` | save presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the above, save every render target of those frames |
| `VICTORY_DUMP_EVERY=1` | dump on a stride instead of at listed frames |
| `VICTORY_DUMP_WORLD=1` | dump only frames that draw a world (>2,000 draws) |
| `VICTORY_DUMP_CBV=1` | log the first words of vertex root constant buffers |
| `VICTORY_GPU_TIMING=1` | time each list; log slow ones with their slowest commands |
| `VICTORY_OPCODE_STATS=<hash>` | emitted instructions by name |
| `VICTORY_SKIP_SLOW_PASSES=1` | bisection aid: drop compute passes on slow frames |
| `VICTORY_LENIENT_MASKS=1` | take untracked lane masks as this wave's own bits |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | drop the draws and dispatches of named pipelines |
| `VICTORY_TEXTURE_BUDGET_MIB=<n>` | cap host texture memory |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_DRED=1` | on GPU failure, the allocation hit and commands in flight |
| `VICTORY_D3D12_WARP=1` | software adapter (slow; cannot reset the display driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | stop translated loops after `n` iterations per lane |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## Upstream projects

This work is derived from, and tracks, open-source console-compatibility
projects: the runtime layer it started from, a sibling runtime with a mature
RDNA2→SPIR-V translator, and the coordination hub for this title's shader census
and shared blockers. Fixes found there are merged in; everything we write lands
here.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
