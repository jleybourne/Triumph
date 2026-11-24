<h1 align="center">
  <br>
  <b>Triumph</b>
  <br>
</h1>

<h4 align="center">A Series-generation console title running natively on Windows, built from your own copy.</h4>

<p align="center">
  <a href="#project-status">Status</a> |
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
> Triumph began as a fork of **VIctory** and still carries its binary and crate names
> (`victory.exe`, `crates/victory-*`). Only the repository is called Triumph. Where this
> tree differs from every upstream it names, it is recorded in the code and in `docs/`, so
> the next person reads the reasoning instead of re-deriving it.

## Project status

**Not playable yet. It loads into the world and executes the world's frames, but the world's
image is black and a world frame takes 45–60 seconds.**

Last measured on the machine below, 2026-11-25.

| What | Status |
|---|---|
| Boot, first-run screens (accessibility, language, settings, legal) | ✅ Works, 30 fps |
| Intro cutscene and loading art | ✅ Plays from start to finish, in order, full colour |
| Start screen, profile, saves (autosave + checkpoint load) | ✅ Works |
| Loading into the world (~50 s) | ✅ Works |
| Input: movement, camera, pause menu (pad and keyboard) | ✅ Works |
| Renderer, in the cinematic stages | ✅ A fully lit 3D image, verified from captured render targets |
| World draws submitted | ✅ **3,656–5,020 draws/frame execute** (up from 2,304 this week) |
| World image | ❌ **Black.** Nothing rasterises: every depth target reads zero and the two 4K scene targets that carry content carry only flat screen-space quads |
| World compute passes | ⚠️ Most execute; the ones whose shaders still do not translate are dropped and logged by name |
| Frame time in the world | ❌ **45–60 s/frame**, which sits on the 60 s driver watchdog — the frame that goes past it loses the device |
| GPU stability | ✅ The 63 s watchdog kill caused by a group barrier in a divergent block is fixed (`removed=0` on the runs that stay under the watchdog) |
| Mesh pipelines | ✅ **0 refused** (was 299, every one dropped for the rest of the run) |
| Device removals from view/pipeline format disagreement | ✅ Fixed: render-target views, pipeline states and clear values agree on one fully qualified format |
| Console wave size (64-lane programs on 32-lane hardware) | ✅ Emulated across two device waves; declared only to programs that read a lane mask or cross lanes |
| The title's deliberate aborts (`mov [0], tag`, `int3`) | ✅ Stepped over and logged instead of ending the process |

### Translator coverage

`victory translate` over the title's 26,084 programs, lenient lane masks on:

```
DXIL  12,887 emitted of 14,027 vertex and pixel programs
       2,354 emitted of  2,582 compute programs
       8,308 emitted of  8,713 mesh programs
```

That is 23,549 of 25,322 (93%). The compute set is the one that matters for the black
world, and its remaining failures cluster into two families:

| count | cause |
|---|---|
| 274 | lane mask read as data |
| 115 | descriptor read only as data |
| 87 | select on an untracked mask |
| 85 | lane mask holding lanes elsewhere, read as data |
| 34 | address in an SGPR that is no root parameter |
| 24 | cross-lane operation outside the entry block |
| 17 | address not from a root parameter |

The mask family (446) and the address family (166) are design changes in the translator, not
missing opcodes.

### What is left, in order

1. **Find out whether the world's mesh programs emit any primitives.** With every draw and
   most dispatches executing and all depth targets still zero, the next measurement is the
   mesh output itself (`SetMeshOutputCounts` and the indices), which separates "no primitives
   emitted" from "positions collapse to one point".
2. **Get a world frame under the watchdog.** 45–60 s per frame is one frame away from a
   device loss on every run; the cost is per-workgroup translated code, so the levers are
   instruction count per lane and 16-bit paths, not scheduling.
3. **Smaller gaps.** Ray-traced bounce light and ray-traced lamp/sun shadows do not run yet,
   so anything the sun does not hit directly is black. Render-target metadata planes, reverb,
   HDR and dynamic resolution are also outstanding.

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

### Commands

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
| `VICTORY_DUMP_CBV=1` | log the first words of vertex root constant buffers |
| `VICTORY_LOG_OPS=1` | name every dispatch and mesh dispatch as it is recorded; the last line before a stall names the program that stopped the card |
| `VICTORY_GPU_TIMING=1` | time each list; log slow ones with their slowest commands |
| `VICTORY_GPU_SLOWLOG=1` | log the GPU time of the slowest command of every submission |
| `VICTORY_GPU_SKIP_MS=<n>` | with the above, stop drawing pipelines whose command costs more than `n` ms |
| `VICTORY_OPCODE_STATS=<hash>` | emitted instructions by name |
| `VICTORY_SKIP_SLOW_PASSES=1` | bisection aid: drop compute passes on slow frames |
| `VICTORY_LENIENT_MASKS=1` | take untracked lane masks as this wave's own bits |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | drop the draws and dispatches of named pipelines |
| `VICTORY_TEXTURE_BUDGET_MIB=<n>` | cap host texture memory (default: dedicated VRAM − 2 GiB) |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_DRED=1` | on GPU failure, the allocation hit and commands in flight |
| `VICTORY_D3D12_WARP=1` | software adapter (slow; cannot reset the display driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | stop translated loops after `n` iterations per lane |

Every build appends crashes and host GPU failures to `crashes.log` in the title's data folder.

## How it works

- **The title's code runs as it is.** The console's CPU is x86-64, so its image is mapped and
  entered, and the console's system modules are reimplemented behind it.
- **Shaders are translated, not recompiled.** Each GPU program the title hands over is
  carved out of its container, disassembled from RDNA2, structured, and emitted as DXIL. The
  translation is cached by content hash, so a second launch does not pay for it.
- **A host pipeline per title pipeline, made off the title's threads.** Driver compiles take
  from milliseconds to seconds; the world's first frame wants over a thousand. They are made
  by workers while the title runs, and a creation the device refuses for want of memory is
  retried with a backoff instead of being given up on.
- **Descriptors are mirrored, not aliased.** The title's descriptor heaps are rebuilt as host
  view heaps, and the fields a shader reads as numbers are published from the host view,
  because the title's own descriptor memory holds a handle record, not a GPU descriptor.

## Want to help?

The two open problems are named, numbered and reproducible on one machine:

- **The black world.** A capture, the draw and dispatch accounting, and the execution of a
  world frame are all one command away (`VICTORY_CAPTURE`, `VICTORY_DUMP_TARGETS`). What is
  needed is the mesh output measured, not another theory.
- **The translator's remaining 7%.** The failure table above is a to-do list; each family is
  a design change with a corpus count that moves or does not.

If you touch the translator, measure the whole corpus before and after and quote both
numbers — a single-gap fix rarely emits a shader, because programs fail on the combination
of limitations.

## Credits

This project is a fork and an accumulation of other people's work. Everything below is used,
read, or ported with thanks.

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its contributors — the
  base of everything here: the loader, the runtime services, the Direct3D 12 back end and the
  shader recompiler. GPL-3.0-or-later; so is this repository.
- **[fxceless/victory-gta6](https://github.com/fxceless/victory-gta6)** — a fork tracking
  upstream main with fixes aimed at rendering the world. Its write-up of the device loss
  introduced by the wave64 update ("a dispatch of them stood on the card until the device was
  reset") identified the group-barrier-in-a-branch hang that cost this tree weeks; the fix
  here is our own code but the diagnosis is theirs. Its configurable host-pipeline wait is
  next to be ported.
- **[ivall/xcl](https://github.com/ivall/xcl)** by ivall and its contributors (GPL-2.0-only) —
  the sibling runtime used as the reference for how this title behaves under another layer,
  and the design reference for wave64-on-wave32: `docs/wave64-on-wave32.md` and
  `crates/xcl/src/gpu/recomp/{upper_lanes,x64_masks,mesh_wave}.rs` specify the lane exchange
  (ballot / readlane / `ds_bpermute` through groupshared memory), note that DPP, `ds_swizzle`
  and `perlane16` never cross a 32-lane half, and keep lane-by-lane masks half-local with a
  liveness pass.
- **[ubiquitarianism/leonida](https://github.com/ubiquitarianism/leonida)** by NotRexed and
  its contributors — the reference translator for the same console generation, whose separable
  modules (`tools/lane_masks.py` for parked masks and masks through the VALU,
  `tools/wave64_halves.py` for partition invariance) are the map this tree's remaining mask
  failures are being fixed against. This README's layout follows that project's.
- **The VIctory contributors whose pull requests are merged here** — notably the raw host views
  for buffers of unknown stride (#704) and `image_sample_d_cl` / `image_gather4_l` (#694),
  which together lifted 61 refused pipelines, and the per-lane phi work (#566).
- **[windows-rs](https://github.com/microsoft/windows-rs)** (MIT) by Microsoft — the Direct3D
  12, DXGI and Win32 bindings.
- **DXC** (DirectX Shader Compiler) — DXIL validation and the container this translator emits.
- **`blake3`**, **`tracing`** and the Rust standard library (MIT / Apache-2.0).

All trademarks and game names belong to their owners. This project is not affiliated with or
endorsed by them.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
