<h1 align="center">
  <br>
  <b>Triumph</b>
  <br>
</h1>

<h4 align="center">A Series-generation console title running natively on Windows, built from your own copy.</h4>

<p align="center">
  <a href="#project-status">Status</a> |
  <a href="#milestones">Milestones</a> |
  <a href="#where-it-runs">Where it runs</a> |
  <a href="#how-to-build">How to build</a> |
  <a href="#how-it-works">How it works</a> |
  <a href="#want-to-help">Want to help?</a> |
  <a href="#credits">Credits</a> |
  <a href="#license">License</a>
</p>

Triumph is a compatibility layer. The title's own x86-64 code runs unchanged inside our
process; Triumph provides the platform underneath it and converts its data — ahead of time
where it can, on first use where it must (shaders). The console's precompiled RDNA2 GPU
programs are translated to DXIL for Direct3D 12, and the title's models, textures and audio
are read from your package. All of this happens **on your PC, from your own copy of the game**.

This project refers to the game it runs as **New Game 6**.

> [!IMPORTANT]
> This repository **does not contain the game, keys or an executable**, and it does not
> explain how to get them. You need your own copy, with its files already readable on disk.
> Nothing here decrypts or works around any protection. Everything the build creates (the
> translated shaders, the runtime folder, your saves) comes from your game and stays on your
> PC. Never share those files.

> [!NOTE]
> Triumph is built on **fxceless/victory-gta6**, which in turn is a fork of **VIctory**. The
> binary and crates keep their `victory` names (`victory.exe`, `crates/victory-*`). Where this
> tree differs from every upstream it names, the reasoning is in the code and in `docs/`.

## Project status

**The world renders, lit and textured, with nothing dropped: about 317-339 ms a frame
in the world (just under 3 fps).**

Measured on the machine below, 2026-11-25, plain launch, no switches set. The CPU
path was 473-733 ms a frame before this round and 265-285 ms now; the frame is bound
by the card.

```
Americas | Victory  316.5 ms/frame  4768 draws/frame
Americas | Victory  338.6 ms/frame  4768 draws/frame
host pipelines refused 0        device removals 0
unhandled exceptions 0          textures on the card
```

Two profiles are selectable (`--console`): `pc`, the default, reports this machine's
own card read from DXGI at launch; `series-x` reports the console's 10 GiB + 3.5 GiB.
Measured on the same room: pc 316-339 ms, series-x 366-380 ms. Reporting a single
memory pool instead of two made the title allocate 23.3 GB and run at 455-470 ms --
the pool shape matters, which is why `pc` keeps Series X's.

| What | Status |
|---|---|
| Boot, first-run screens (accessibility, language, settings, legal) | ✅ Works, 30 fps |
| Intro cutscene and loading art | ✅ Plays from start to finish, in order, full colour |
| Start screen, profile, saves (autosave + checkpoint load) | ✅ Works |
| Loading into the world | ✅ Works |
| **World renders** | ✅ **Yes** — geometry, characters, textures, sky, sun, HUD; verified from the presented frame |
| Input: movement, camera, pause menu (pad and keyboard) | ✅ Works |
| World draws submitted | ✅ 4,934–6,990 draws/frame |
| Host pipelines | ✅ **0 refused.** Every pipeline the world asks for is created (2,428 made) |
| Frame time in the world | ⚠️ **316–339 ms/frame (~3 fps)** on a plain launch — the budget is ~16 ms |
| Ray-traced lighting | ⚠️ The structure builder **runs** (53/53 programs) and shadows resolve; the second-order terms — bounce on interior walls, light shafts — are thin because four volumetric and water programs still do not translate. See `docs/remaining-shaders.md` |
| Old note | Everything the sun does not hit directly was black: the title's ray-traced bounce light and ray-traced lamp/shadow passes do not run yet |
| The first gameplay room after the cutscene hands over | ⚠️ Sometimes not drawn |
| Memory in play | ⚠️ About 24 GB in use |
| GPU stability | ✅ No device loss in normal runs |
| Deliberate aborts in the title (`mov [0], tag`, `int3`) | ✅ Stepped over and logged instead of ending the process |

### Translator coverage

`victory translate` over the title's 25,694 programs:

```
DXIL  14,156 emitted of 14,247 vertex and pixel programs
       2,668 emitted of  2,682 compute programs
       8,762 emitted of  8,765 mesh programs
```

**25,586 of 25,694 (99.58%).** Compute — the set that builds the frame in a GPU-driven title —
is 2,668 of 2,682.

What the 108 refusals are, by count of hits:

| hits | reason | first program |
|---|---|---|
| 195 | an address in an SGPR that is no root parameter | `GS_VoxelizeGeneric` |
| 40 | a descriptor chosen by control flow | `PS_TexturedLit_ReflectionMap` |
| 40 | a descriptor read only as data | `PS_PuddleDetect` |
| 10 | a hardware SGPR input | `PS_PedTexturedCutOutTint_Zero` |
| 8 | the first half of an interpolation | `PS_DeferredTextured` |
| 5 | `v_cvt_f32_f64` | `CS_TransmissionVolumetricsSubmerged` |
| 2 | a wave sum of 16-bit floats where not every lane runs | `compute_graph` |
| 6 | single instances (opaque lane mask, `v_permlanex16_b32`, descriptor address or format as data) | `PS_VoxelizeVisiblity`, `CS_WaterTransmittanceOpticalDepth` |

Eight of these reach the world; the rest are voxelisation and the reflection
families. `docs/remaining-shaders.md` names each of the eight and what
feature it removes.

### What is left, in order

1. **Frame time.** About 260 ms is 16× the budget. The cost is per-workgroup translated code, not
   scheduling: instruction count per lane, 16-bit paths, and the wave64-on-wave32 exchange.
   The clean-up passes over finished modules already took about 43% off the host instruction
   count for ~20 ms; the rest is the same fight.
2. **Indirect light.** Interiors are lit by direct and emissive light only (panels, sun through
   a door, contact shadows) — the ray-traced bounce light and lamp/sun shadows are the missing
   half. They live behind one all-or-none gate, and it is closed by a small, enumerated list:
   - **3 of 53** ray-tracing structure-builder compute programs do not translate
     (`39321a95223001ed`, `77360df21e12ea43`, `9fc111d3be79364f`), and the builder runs all
     stages or none.
   - **4 compute programs** are rejected by `CreateComputePipelineState` with
     `E_INVALIDARG` (`2da4beb386509b77`, `369cbe906ea72764`, `02384a60fda6f2d0`,
     `2cc4c197ee29e291`) — a root-signature/shader disagreement, the same class as the
     declared-wave-size bug. They are retried eight times and then dropped, and several
     material passes are among the ones dropped.
3. **The first gameplay room** is sometimes not drawn after the cutscene hands over.

### What enabling those passes costs, measured

Turning on the extra compute/material passes (the 32-lane split of console wave64 programs) is
the obvious way to get surface materials and the ray-traced light, and it is not yet safe. Four
configurations were measured on the test machine; every one that runs those passes aborts the
title on its own validity check (`mov [0], tag`), with no device removal and no host error:

| configuration | refusals | result |
|---|---|---|
| default (those passes refused) | 23 | runs, 4-6 fps, `removed=0` |
| `VICTORY_WAVE_WIDE=1` | 9 | aborts |
| `... WAVE_WIDE=1` skipping the three newly-unlocked compute programs | 9 | aborts |
| `... WAVE_WIDE=1` skipping the newly-unlocked pixel program | 9 | aborts |
| `... WAVE_WIDE=1 VICTORY_WAVE_LOCAL=idiom` (only the exact split) | 4 | aborts |

So the abort is not produced by one identifiable pass, and not by the approximate split alone:
skipping candidates changes nothing, and the exact-only form still aborts. The passes' translated
output is wrong in some way that is not yet isolated, which is why they stay off by default and why
"missing textures" and "missing indirect light" appear together. The next person should not
re-run the bisected skip lists above — they are already done.

## Milestones

Every change that moves the end result is committed here with its measurement.

| Date | Milestone | Evidence |
|---|---|---|
| 2026-11-25 | **Interiors light up.** Direct and emissive light, contact shadows and the HUD in the first gameplay room; the missing half is the ray-traced indirect pass | 6,890 draws/frame, 4 fps, 280.9 ms, `removed=0` |
| 2026-11-25 | **The lighting blockers are named, not guessed.** The all-or-none ray-tracing builder gate was made to print what it is missing | 3 of 53 builder programs untranslated; 4 compute pipelines refused `E_INVALIDARG` |
| 2026-11-25 | **Frame time nearly halved.** `VICTORY_RELAY=2` sends submissions ahead of the wait for the card and lands their results as it finishes | 478.6 → 252.9 ms/frame at 4,931 draws/frame; `removed=0` |
| 2026-11-25 | **The world renders.** Base moved to the fxceless fork's tree, whose meshlet-culling, view-kind, mip-level and implicit-sample fixes are what make the world draw at all | 5,057 draws/frame, 2 fps, `removed=0` |
| 2026-11-25 | **A refused pipeline is retried, not abandoned.** `CreatePipelineState` returning `E_OUTOFMEMORY` used to be cached as "cannot be made" for the whole run | draws/frame 3,972 → 5,060; frame time 749.7 → 478.6 ms |
| 2026-11-25 | **The 63 s watchdog kill is gone.** A group barrier in a block only some lanes reach hangs the device; it is left out of those blocks | `removed=0` on runs that previously lost the device every time |
| 2026-11-25 | **Mesh pipelines stop being dropped.** 299 mesh pipelines were refused for memory, and the null-UAV guard read deferred views and dropped every world mesh dispatch | refusals 299 → 0; world draws/frame 2,304 → 5,020 |
| 2026-11-25 | **Translator coverage** on the older base reached 93% and its compute gaps were tabulated | 23,549 of 25,322 emitted |

## Where it runs

Development and testing are on one machine. Every number in this README comes from it.

| | |
|---|---|
| GPU | NVIDIA RTX 4070 Ti, 12 GB, driver 610.88 — 32-lane waves, no 64-lane support |
| CPU | Intel i7-10700KF — no CLWB/CLZERO; the runtime emulates the console's AMD-only instructions |
| Memory | 64 GB |
| OS | Windows 11, build 26300 |
| Display | 2194×1234 (the game window is placed 16:9 on the right, so it does not cover the editor) |
| Toolchain | Rust 1.98.1, MSVC 14.51.36231, DXC 10.0.26100, Windows SDK 10.0.26100 |

Windows only. Linux and macOS are not targets.

## How to build

Requirements: Windows 10/11 x64, AVX2, a Direct3D 12 GPU with shader model 6.6 and mesh
shaders, Rust (pinned in `rust-toolchain.toml`). The title needs several GiB of Windows
commit at startup — check **Committed** in Task Manager if `crashes.log` reports error 1455.

```powershell
copy victory.example.toml victory.toml     # then set your paths (outside the repo)
cargo test --workspace                     # needs no game data
cargo run -p victory -- config             # check your paths

cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log` live under the data
folder (default `%LOCALAPPDATA%\victory\data`); derived data under the cache folder.

Input: XInput pads. With none connected the keyboard acts as a pad while the title's window
has focus:

| Keys | Pad |
|---|---|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

### Diagnostics

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
| `VICTORY_GPU_TIMING=1` | time each list; log slow ones with their slowest commands |
| `VICTORY_GPU_SLOWLOG=1` | log the GPU time of the slowest command of every submission |
| `VICTORY_GPU_SKIP_MS=<n>` | with the above, stop drawing pipelines whose command costs more than `n` ms |
| `VICTORY_PIPELINE_WAIT_MS=<n>` | waiting allowed per frame for host pipelines (default 500) |
| `VICTORY_OPCODE_STATS=<hash>` | emitted instructions by name |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | drop the draws and dispatches of named pipelines |
| `VICTORY_TEXTURE_BUDGET_MIB=<n>` | cap host texture memory |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_DRED=1` | on GPU failure, the allocation hit and commands in flight |
| `VICTORY_D3D12_WARP=1` | software adapter (slow; cannot reset the display driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | stop translated loops after `n` iterations per lane |
| `VICTORY_WAVE_WIDE=1` | the two-device-wave form for 64-lane programs (off by default: its barriers sit in branches) |
| `VICTORY_RELAY=2` | send submissions ahead of the wait and land results as the card finishes: **the frame-time win, 478 → 253 ms** |
| `VICTORY_LATE_WAIT=<n>` | send up to `n` submissions ahead of the wait |
| `VICTORY_WIDE_BACK=1`, `VICTORY_CARD_BUFFERS_MIN_MB=<n>`, `VICTORY_CARD_BUFFERS_MB=<n>` | larger in-card buffers, back-ended |

Every build appends crashes and host GPU failures to `crashes.log` in the title's data folder.

Measured caution: `VICTORY_RELAY=2` on its own is stable here, but the combination
`VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1 VICTORY_CARD_BUFFERS_MIN_MB=1 VICTORY_CARD_BUFFERS_MB=1024`
lost the device to a reset (`0x887A0007`) while still in the menus on this machine. Change one at a
time and keep the run that survives.

## How it works

- **The title's code runs as it is.** The console's CPU is x86-64, so its image is mapped and
  entered, and the console's system modules are reimplemented behind it.
- **Shaders are translated, not recompiled.** Each GPU program the title hands over is carved
  out of its container, disassembled from RDNA2, structured, and emitted as DXIL. The
  translation is cached by content hash, so a second launch does not pay for it.
- **A host pipeline per title pipeline, made off the title's threads.** Driver compiles take
  from milliseconds to seconds; the world's first frame wants over a thousand. They are made
  by workers while the title runs, and a creation the device refuses for want of memory is
  retried with a backoff instead of being given up on.
- **Descriptors are mirrored, not aliased.** The title's descriptor heaps are rebuilt as host
  view heaps, and the fields a shader reads as numbers are published from the host view,
  because the title's own descriptor memory holds a handle record, not a GPU descriptor.

## Want to help?

Two problems are named, numbered and reproducible on one machine:

- **Frame time.** Per-command GPU timings come out of `VICTORY_GPU_TIMING`; the slowest
  commands name their shader hash, and a hash is resolvable to a source program with
  `shader-info --verbose`. What is needed is work on translated code, not on settings —
  halving the resolution does not move these numbers.
- **Light in shadow.** The ray-traced passes are the missing image. Their structure-builder
  programs already translate; the hardware ray test is the gap.

If you touch the translator, measure the whole corpus before and after and quote both
numbers. A single-gap fix rarely emits a shader, because programs fail on the combination of
limitations.

## Credits

This project is a fork of a fork and an accumulation of other people's work. Everything below
is used, read, or ported with thanks.

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its contributors — the
  base of everything here: the loader, the runtime services, the Direct3D 12 back end and the
  shader recompiler. GPL-3.0-or-later; so is this repository.
- **[fxceless/victory-gta6](https://github.com/fxceless/victory-gta6)** by fxceless — the tree
  this repository is now built on, and the reason the world renders at all: meshlet culling
  reading its scene buffers as the right kind of view, per-draw descriptor-table exhaustion,
  light read back through empty copies for every mip level and layer, gathers at a named mip
  level, and the invalid DXIL that implicit-level samples produced in 730 compute programs.
  Its write-up of the wave64 device loss ("a dispatch of them stood on the card until the
  device was reset") also identified the group-barrier-in-a-branch hang fixed here.
- **[ivall/xcl](https://github.com/ivall/xcl)** by ivall and its contributors (GPL-2.0-only) —
  the sibling runtime used as the reference for how this title behaves under another layer,
  and the design reference for wave64-on-wave32: `docs/wave64-on-wave32.md` and
  `crates/xcl/src/gpu/recomp/{upper_lanes,x64_masks,mesh_wave}.rs` specify the lane exchange
  (ballot / readlane / `ds_bpermute` through groupshared memory), note that DPP, `ds_swizzle`
  and `perlane16` never cross a 32-lane half, and keep lane-by-lane masks half-local with a
  liveness pass.
- **[ubiquitarianism/leonida](https://github.com/ubiquitarianism/leonida)** by NotRexed and its
  contributors — the reference translator for the same console generation, whose separable
  modules (`tools/lane_masks.py` for parked masks and masks through the VALU,
  `tools/wave64_halves.py` for partition invariance) are the map the remaining mask failures
  were read against. This README's layout follows that project's.
- **The VIctory contributors whose pull requests are merged here** — notably the raw host views
  for buffers of unknown stride (#704) and `image_sample_d_cl` / `image_gather4_l` (#694),
  which together lifted 61 refused pipelines, and the per-pane phi work (#566).
- **[windows-rs](https://github.com/microsoft/windows-rs)** (MIT) by Microsoft — the Direct3D
  12, DXGI and Win32 bindings.
- **DXC** (DirectX Shader Compiler) — DXIL validation and the container this translator emits.
- **`blake3`**, **`tracing`** and the Rust standard library (MIT / Apache-2.0).

All trademarks and game names belong to their owners. This project is not affiliated with or
endorsed by them.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

### Correction: the shared-pipeline numbers

The commit that shares one host pipeline between identical requests first
quoted "pipeline creations 256 -> 0, draws 502 -> 4,938". Those figures came
from a run that used the previous binary: the copy of the new one onto the
path the runner launches failed with `Device or resource busy`, so the fix
was not running. Re-measured with the new binary in place: pipeline creations
in the last 256 operations 256 -> 41, and the run still ends on the same fault
at +0x23cf683. The sharing reduces how often a driver pipeline is compiled;
it does not remove the fault, which is a separate defect.
