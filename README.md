<h1 align="center">Triumph</h1>

<p align="center"><b>An Xbox Series title running natively on Windows, on an NVIDIA card,<br>
and the work of getting its world to 30 frames a second.</b></p>

Triumph is a fork of [VIctory](https://github.com/Marvisak/VIctory), a compatibility layer that runs
an Xbox Series title's own x86-64 code on Windows and supplies everything underneath it: the console's
system calls, its memory model, and its GPU, translated to Direct3D 12. This fork has one job: **make
the 3D world playable on one specific PC without losing picture or correctness.**

> [!IMPORTANT]
> This repository contains no game files, code or assets. You need your own copy of the title.
> Nothing here is affiliated with or endorsed by its developer or publisher.

## Where it stands

Measured on one machine (i7-10700KF, RTX 4070 Ti 12 GB, 64 GB RAM, Windows 11), world scene,
diagnostics build, confirmed launch settings. Every row links to its run in
[docs/runs.md](docs/runs.md).

| | now | target |
|:--|:--|:--|
| **Frame rate** | **7.3 fps · 136.6 ms** median, 8.0 fps best ([row 26](docs/runs.md)) | 30 fps |
| **Scene drawn** | **4,706 draws** a frame, 344 dropped (was 3,962 and ~478) | 0 dropped |
| **Pipelines refused** | **7** (was 311) | 0 |
| **Crashes** | 0 in the runs above | 0 |
| **Resolution** | 2560x1440 scene, upscaled to 3840x2160 | lower scale + better upscaler |

Every result is reported as **fps · ms · draws · dropped** together. A faster frame that draws less
of the scene is not a win.

## Why it is slow, and what fixes it

The console has one memory that the CPU and the GPU share. A PC does not, so this layer currently
**keeps a copy of the title's graphics memory on the card and syncs it every frame**: it uploads what
the title changed, hashes it, and copies back what the GPU wrote. In a ~130 ms frame:

```
title's render thread     6 ms of work, then ~68 ms waiting for us in present
our submit thread         74 ms: copies 23.5 ms (72 MB) + copy-back 42.8 ms (56 MB) + the rest
the GPU                   idle ~44 ms; of ~71 ms busy, 26 ms is our own hashing, ~45 ms the game
```

The game is not the bottleneck. Our memory mirroring is. The plan, in order:

| | step | why |
|:--:|:--|:--|
| 1 | **Zero-copy memory**: the GPU works on the title's memory directly | removes ~65 ms of CPU work and 26 ms of GPU work a frame |
| 2 | **Build tuning**: AVX2 (`target-cpu=native`), link-time optimisation | faster compares and copies, free |
| 3 | **Lower render scale + DLSS** in place of the title's FSR | once the GPU is the limit, this halves its work |
| 4 | **NVIDIA wave work**: 64-lane console waves run natively as 32-lane pairs | the last 7 refused pipelines; fewer shader instructions |
| 5 | **DLSS Frame Generation** | last: smoother, not faster |

## Recent findings

- **311 pipelines were refused because of an earlier "fix".** The D3D12 debug layer showed the
  shaders index the sampler heap directly, and the flag allowing that had been removed. Restoring it
  took refusals from 311 to 7 and the scene from 3,962 to 4,706 draws ([row 26](docs/runs.md)).
- **`fsrQuality=4` works now.** It was black only because its passes were among the refused
  pipelines ([row 27](docs/runs.md)). It is no faster yet, because the GPU is not the bottleneck.
- **Measurements need a quiet machine.** A background build during one run made the same build read
  2.3 fps instead of 7.3. Runs are only trusted with nothing compiling.

What has been tried and rejected, and why, is kept in [docs/runs.md](docs/runs.md) so nothing is
paid for twice.

## Running it

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with shader model 6.6 and mesh
shaders, Rust (pinned in `rust-toolchain.toml`), and several GB of free Windows commit.

```powershell
copy victory.example.toml victory.toml        # set your game and data paths (outside the repo)
cargo test --workspace                        # needs no game data
cargo run -p victory -- config                # check your paths
cargo build --profile diag --features diagnostics -p victory
target\diag\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the frame rate. With no pad connected, the keyboard acts as one:
W A S D / I J K L sticks, arrows D-pad, Enter / Backspace / X / Y face buttons, Q / E shoulders,
Z / C triggers, Esc / Tab Menu / View.

Useful diagnostics switches (diagnostics builds only):

| variable | effect |
|:--|:--|
| `VICTORY_D3D12_DEBUG=1` | D3D12 debug layer; names why a pipeline or command is rejected |
| `VICTORY_D3D12_DRED=1` | on a GPU fault, what was running |
| `VICTORY_CAPTURE=<dir>` | saves presented frames |
| `VICTORY_LOG=<target>=debug` | more logging |

The full list is in upstream's README.

## Documentation

| file | what it holds |
|:--|:--|
| [CLAUDE.md](CLAUDE.md) | the working handoff: measured state, blockers, rules, order of work |
| [docs/runs.md](docs/runs.md) | every measured run, including the rejected ones and why |
| [docs/settings.md](docs/settings.md) | the title's own settings file and what each change measured |
| [docs/per-lane.md](docs/per-lane.md) | the wave64 / per-lane translation problem |
| [docs/architecture.md](docs/architecture.md) | how the layer is put together |

## Credits

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and contributors is the base of
  everything here: loader, runtime services, the Direct3D 12 back end, the shader recompiler.
- **[XCL](https://github.com/ivall/xcl)** by ivall and contributors is a design reference: its unified
  memory and wave64-on-wave32 designs. XCL is GPL-2.0-only and this fork is GPL-3.0, so its ideas
  are reimplemented here, never copied.
- The guard over every view of a memory page is by **myokex** (victory-amd `c6dc4d8`), adapting
  pull request #115 by Spexivus, its correction by joroyt and part of VIctory #1122 by tightfall. The
  read-beside fix for kernel reads is by **Joshua Odom** (`10ea38e`).
- Developed with [Claude Code](https://claude.com/claude-code) and Hermes Agent. Figures on this page
  were measured on the machine described, not estimated.

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE).
