<h1 align="center">Triumph</h1>

<p align="center"><b>Running an Xbox Series title natively on Windows — and getting its world<br>
from 3.4 frames a second to a number worth playing.</b></p>

### Status at a glance

One row per part of the work. The mark on the left is that row's state; the two rightmost columns are
where it stands **now** — with the numbers actually measured — and what done looks like. Nothing in this
table is an intention: if a number is there, it came from a run whose log is named at the bottom of this
page.
| | status | what it is | now | target |
|:--:|:--|:--|:--|:--|
| 🟢 | **Frame rate** | release build, world scene, 4,710 draws a frame | **7.7 fps** · 130 ms · 348 dropped | 30 fps |
| 🟢 | **Shaders** | console RDNA2 programs translated to DXIL | **25,586 / 25,694** · 99.58% | 25,694 / 25,694 |
| 🟢 | **Stability** | crashes and device removals across every run measured | **0** | 0 |
| 🟢 | **Pipeline library** | pipelines loaded from disk instead of compiled | **2,495 loaded · 0 compiled** | unchanged |
| 🔄 | **Copy-back** | shader writes returned to the title's memory | 73.9 ms · 64.7 MB · 0.88 GB/s | ≤ 20 ms |
| 🔄 | **Per-draw GPU** | state work the layer issues for each draw | 90.8 ms · ~19.8 µs a draw | ≤ 20 ms total |
| 🔄 | **The last 108 shaders** | one shape: a masked wave reduction | approximated by `VICTORY_WAVE_MASKED` | exact |
| ⏳ | **Double-buffered copies** | frame N+1 prepared while frame N draws | 141 ms frame, card 64% busy | ~95 ms → 10.5 fps |
| ⚠️ | **Dropped draws** | draws lost to pipelines still loading | 344–397 a frame (8% of scene) | 0 |
| ❌ | **Shadowed areas** | parts of the picture that render black | upstream issue, untouched here | fixed |

### Reading the marks

Shorthand used in the table above and through the rest of this page. **Status** marks say what state a
thing is in; **confidence** marks say how sure we are of a forecast.
| mark | means | mark | means |
|:--:|:--|:--:|:--|
| ✅ | works, measured | 🟢 | high confidence |
| ❌ | broken or rejected | 🟡 | medium confidence |
| ⚠️ | partial | 🔴 | low confidence |
| 🔄 | in progress | ⏳ | not started |

---

A fork of [VIctory](https://github.com/Marvisak/VIctory) — a compatibility layer that runs an Xbox
Series title natively on Windows, from files the user supplies. This fork tracks upstream `main` and
exists for one purpose: **make the 3D world run at a playable frame rate on one particular machine,
without losing picture or correctness.**

> [!IMPORTANT]
> **This repository contains no game files, code or assets.** You need your own copy of the title.
> Nothing here is affiliated with or endorsed by its developer or publisher.

## Shaders

Every GPU program the title ships has to be decoded from the console's RDNA2 form and re-emitted as
DXIL before the card will run it. This is the count, measured over the title's own shader archive:

```
  25,586 / 25,694  ██████████████████████████████████░░  99.58%
```

| stage | translated | of | |
|:--|--:|--:|--:|
| vertex + pixel | 14,156 | 14,247 | `█████████████████████████████████░` 99.36% |
| compute | 2,668 | 2,682 | `█████████████████████████████████░` 99.48% |
| mesh | 8,762 | 8,765 | `██████████████████████████████████` 99.97% |

The **108 refused** programs are all one shape — a wave reduction over lanes that are not all
running. They run today with `VICTORY_WAVE_MASKED=1` (approximately); the exact fix is being written
now and needs each lane's execution mask carried as data. See `docs/per-lane.md`.

## Where we stand now

One machine: Windows 11, Intel i7-10700KF, NVIDIA RTX 4070 Ti 12 GB, 64 GB RAM, Series X profile.
World scene, frames counted only at **1,200 draws or more**, after the load phase settles.

| build | draws | fps | ms a frame | dropped |
|---|---|---|---|---|
| **release** | 4,710 | **7.7** | **130.0** | 348 |
| diagnostics | 4,525 | 6.9 | 145.9 | 356 |
| diagnostics · 960×540 internal | 4,579 | 7.3 | 140.9 | 344 |
| menus | 8–32 | 30.0 | 33.3 | 0 |

Since the first measurement this session the tree has gone from **3.4 fps / 295 ms** (hand-merged
base) to **7.7 fps / 130 ms** — a 2.3× frame-rate gain and a 37% cut in cost per draw
(0.051 ms → 0.032 ms), almost all of it from adopting upstream's base and its measured switch set.

## What a frame is made of

| part | ms | what it actually is |
|---|---|---|
| the card, busy | 90.8 | ~19.8 µs of GPU work per draw |
| preparing | 95.0 | includes copies: 75.4 ms moving 114.9 MB in **11,542 copies** |
| copying back | 73.9 | 64.7 MB at 0.88 GB/s |
| pipeline passes | 55.2 | 2,495 pipelines **loaded** from library, none compiled |

These overlap into the 130–141 ms frame. Two facts organise everything below:

- **The card is only 64% busy**, so the frame is longer than either half; perfect overlap alone
  would give ~95 ms (10.5 fps).
- **The card is not fill-rate bound**: rendering at a quarter of the pixels saves 4%.

## What's working

✅ **The world renders and is playable.** Full scene draws at 4,500–6,900 draws a frame.
✅ **Shader coverage 99.58%** (see [Shaders](#shaders)). The 108 refused programs are one shape and
  `VICTORY_WAVE_MASKED=1` runs them approximately.
✅ **No crashes or device removals** in the runs measured here (0 faults across every run measured).
✅ **Load → world pipeline library** works: 2,495 pipelines loaded, **0 compiled** per run.
✅ **Diagnostics are trustworthy now**: GPU-busy, per-copy counts, descriptor-sync timing, and a
  per-program instruction census (1,433 programs × 26 instruction kinds).

## What isn't working

| | problem | measured cost | state |
|---|---|---|---|
| ❌ | Per-copy overhead in host copies | **63 ms** of a 141 ms frame | understood, fix measured as ineffective |
| 🔄 | Copy-back throughput | 73.9 ms at 0.88 GB/s | fix wired in production, **unmeasured** |
| 🔄 | GPU per-draw cost | 90.8 ms total, ~19.8 µs/draw | three dedup fixes written, **unmeasured** |
| ⚠️ | 356 draws dropped per frame | 8% of the scene | caused by pipelines not ready |
| 🔄 | The last 108 shaders | 0.42% of programs | exact fix in progress (per-lane model) |
| ❌ | Shadowed areas render black | — | upstream issue, untouched here |

## Findings

1. **A frame rate alone is not a measurement.** Frames that drop draws look faster because the
   scene is smaller: the "9 fps" seen early on was a 744-draw frame that dropped 522, against a
   full scene of ~4,500. Every number here is reported as fps · ms · draws · dropped.
2. **A 300-second run cannot measure this title.** The world spends minutes in pipeline loads;
   measurement starts after the spikes settle, so runs are 600 s.
3. **The GPU figure was invisible, not zero.** Under async submission the old frame line read
   `0.0 on the GPU`; the card clock shows **90.8 ms**. The card is nearly two thirds of the frame.
4. **The copy path is overhead-bound, not bandwidth-bound.** 11,542 copies averaging 10.2 KB cost
   6.53 µs each, where the bytes alone are 1.04 µs — **5.5 µs of overhead per copy**.
5. **The 11,542 copies are one per distinct buffer**, not adjacent spans, so joining neighbours
   cannot help (measured: copy count unchanged).
6. **The arithmetic in translated shaders is not waste.** `arith` is 44.6% of instructions and
   provably 0% redundant; the kinds divergence work removes are 16.5%, and `guard_select`, the one
   per-lane EXEC branches would replace, is 0.64%.
7. **Cost is concentrated**: the top 100 programs carry 35.7% of all instructions, and the top ten
   are all compute programs of ~49,000 instructions.

## Investigations, and what they ruled out

| tried | result | verdict |
|---|---|---|
| Adopt upstream base + its measured switch set | 3.4 → 6.9 fps | ✅ **adopted** |
| `VICTORY_HEAP_SYNC_TABLES=1` (narrower heap scan) | 6.0 fps / 167 ms | ❌ rejected — slower |
| `VICTORY_MERGE_RUNS` (join contiguous copy ranges) | copy count unchanged | ❌ rejected |
| `VICTORY_COPY_MERGE` (join runs of adjacent copies) | 11,336–11,795 vs 11,542 | ❌ rejected — not adjacent |
| `VICTORY_EARLY_WORLD=1` (load every pipeline at creation) | 600 s run never reached the world: max 33 draws | ❌ rejected — starves the load phase |
| 960×540 internal (`VICTORY_DISPLAY_MODE`) | +4% | ❌ rejected — not fill-rate |
| Identity-element shortcut for refused shaders | covers 0 of 108 (all are sum) | ❌ rejected — proven empty |
| Per-lane divergence work as a frame-rate lever | caps out at 16.5% of instructions | ⚠️ deprioritised for speed, kept for coverage |
| Zero-copy / resident heaps (study only) | the only route to 33 ms GPU-side | ⚠️ deferred, large |

## What we're working on

1. 🔄 **Copy-back span merge** — written and wired in the production path (gap ≤ 64 bytes, in-flight
   ranges fall back), **measurement pending**. Target: 73.9 ms → 20–30 ms.
2. 🔄 **Per-draw state dedup** — three switches: skip already-bound target/depth transitions, dedupe
   repeated root-argument and descriptor-table sets, rebind vertex buffers only on change.
   Estimated 12–24 µs per draw against a ~19.8 µs budget. **Measurement pending.**
3. 🔄 **Exact translation of the last 108 shaders** (`VICTORY_EXEC_DATA`) — carry each lane's EXEC
   mask as data and if-convert the divergent region, so a reduction no longer needs lanes to be
   co-resident. Acceptance: a numeric test that matches hand-computed console results.
4. ⏳ **Double-buffering the host copies** — so frame N+1 prepares while frame N draws. Target:
   141 → ~95 ms (10.5 fps) with no GPU change.

## What we think we can achieve

Honest arithmetic from the measurements above, not optimism:

| milestone | frame | fps | confidence |
|---|---|---|---|
| copy-back + per-draw dedup land and measure well | ~95–110 ms | **9–10.5** | 🟢 high |
| + double-buffering (perfect overlap) | ~85–95 ms | **10.5–12** | 🟡 medium |
| + deeper per-draw GPU work (state, tables, shaders) | ~55–65 ms | **15–18** | 🟡 medium — the GPU must halve |
| 30 fps | 33 ms | **30** | 🔴 **low — needs the GPU's 90.8 ms to fall ~4×** |

**Timeframe, on one developer plus agent assistance:** the three host-side items (1, 2, 4) are days
of work each and carry the frame to ~10–12 fps. The GPU per-draw reduction is 1–2 weeks of
iterative measurement. **30 fps is not a schedule item yet** — it depends on a per-draw mechanism
that has been measured (20 µs/draw) but not yet found a lever, and it would additionally need the
per-lane model or a comparable cut in emitted instructions.

## Reproducing a measurement

```bash
# build
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --profile diag --features diagnostics -p victory --target-dir target/vd3
cargo build --release -p victory

# run (600 s: the world needs minutes to settle past the pipeline loads)
run-fx.ps1 -Diag -Seconds 600        # diagnostics, prints the full frame line
run-fx.ps1 -Seconds 600              # release, the playable number

# read a run
#   log: victory-run-<time>.log.err
#   keep frames with draws >= 1200, and report fps · ms · draws · dropped together
#   counters that matter: copies made in place (count, MB), copying back (ms, MB),
#   GPU busy ms, passes ms, dropped without a host pipeline
```

Switches used for the numbers above: `VICTORY_ASYNC_SUBMIT=1 VICTORY_QUEUE2=1
VICTORY_INDIRECT_WINDOW=1 VICTORY_TEXTURE_NO_WAIT=1 VICTORY_MERGE_GAP=64 VICTORY_MERGE_RUNS=4096
VICTORY_STAGING_POOL=0 VICTORY_COPY_PLAIN=1 VICTORY_JOIN_PARTS=1 VICTORY_BACK_OWN_VIEW=1
VICTORY_INDIRECT_GATHER=1 VICTORY_CARD_REHASH_RUNS=1 VICTORY_CARD_WINDOWS_MB=2048
VICTORY_CARD_BUFFERS_MIN_KB=64 VICTORY_CARD_BUFFERS_MB=1280 VICTORY_TEXTURE_BACK=1
VICTORY_CARD_HASH_LEN=1 VICTORY_FILLS_PLAIN=1 VICTORY_STEP_INDEX=1`

## Documentation

- `docs/frame-budget.md` — the measurements, the traps that produced wrong numbers, and the rules
  that keep them honest.
- `docs/shader-cost.md` — the instruction census: what translated programs cost and where.
- `docs/pipeline-stalls.md` — why the world stalls on pipeline *loads* and not compiles.
- `docs/per-lane.md` — why the last refusals need a model change, not a patch.

## Credits

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its contributors is the base of
  everything here: loader, runtime services, the Direct3D 12 back end, the shader recompiler.
  GPL-3.0-or-later, so is this fork. Upstream's history is kept intact below.
- **[XCL](https://github.com/ivall/xcl)** by ivall and its contributors (GPL-2.0-only) — reference
  for how the title behaves under another layer. Fixes inspired by it were written anew here.
- The guard over every view of a memory page is code by **myokex** (victory-amd, commit `c6dc4d8`),
  adapting pull request #115 by Spexivus, its correction by joroyt and part of VIctory #1122 by
  tightfall. The read-beside fix for kernel reads is by **Joshua Odom** (commit `10ea38e`).
- Parts of this fork's work were developed with [Claude Code](https://claude.com/claude-code) and
  with Hermes Agent subagents; every change in this README was measured on the machine described.

## License

GPL-3.0-or-later, inheriting VIctory's licence. See `LICENSE`.
---

> Upstream's README follows, kept intact for reference.

# VIctory

Runs the title (Xbox Series X|S) natively on Windows, from files
the user supplies.

The title's x86-64 code runs unchanged inside our process; VIctory supplies
the platform underneath it and converts the title's data, ahead of time where
it can and on first use where it must (shaders). See
[docs/architecture.md](docs/architecture.md).

## Status (2026-11-23)

**Not playable yet.** The title boots, runs its menus at its own 30 fps and
loads the world, where rendering fails.

| Stage | State |
|-------|-------|
| Boot, first-boot screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video with sound | works |
| Start screen, profile and saves | works |
| Loading into the world (about 50 s) | works |
| The world (first mission's intro) | reached and runs with indirect draws; its title card renders, the scene after it is still dark; 0.5-1.8 s per frame |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `VICTORY_D3D12_WARP=1`, which renders on the software adapter.

What each part does today and what it still needs:

| Subsystem | Today | Needs |
|-----------|-------|-------|
| Loader | Runs the title's executable and its DLLs in our process; crash reports name the faulting code and are kept in `crashes.log` | – |
| Runtime services | One signed-in offline user, licences, packages, saves that survive a kill at any step, text entry dialog | Speech is silent; the dialog and relaunch are untested live |
| Network | Offline by design: the title cannot reach the network | – |
| Files and streaming | Archives and streamed data read, zlib and BCPack decoded in parallel | Loading is limited by writes into title memory |
| Input | XInput pads, keyboard, keyboard stand-in pad | Trigger rumble |
| Audio | Menu, video and loading sound; XMA voices and Opus streams decode | Reverb is silent; Opus results unverified against the title |
| GPU: menus | Draws, compute and presentation on host D3D12; only changed title memory is copied | Frame pacing drifts on one screen in release builds |
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes, indirect execution (arguments clamped), pipelines made by worker threads and kept on disk | Most of the scene is still missing (untranslated shaders drop draws); frames take seconds; real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`).

The title also needs several GiB of available Windows commit at startup.
Check **Committed** under **Task Manager → Performance → Memory**. If
`crashes.log` reports Windows error 1455, close other applications or increase
the Windows paging file size, then retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo test --workspace                   # needs no game data
cargo run -p victory -- config           # check your paths
```

Run the title:

```powershell
cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\victory\data`), derived
data under the cache folder.

Input: XInput pads. With none connected the keyboard is a pad while the
title's window has focus:

| Keys | Pad |
|------|-----|
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

Logging: `VICTORY_LOG=victory_loader=trace,info` (a release build logs
info and above).

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `VICTORY_PIPELINE_WAIT_MS=24` | reduce the per-frame wait for background pipeline creation from the default of 500 ms. A draw can be omitted until its pipeline is ready |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `VICTORY_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
