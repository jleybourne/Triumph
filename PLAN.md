<h1 align="center">🗺️ The plan</h1>

<p align="center"><i>From 7.9 fps to a stable 30, built from what was measured, researched and designed.<br>
Updated every 30 minutes while work is running. The latest entry is at the bottom.</i></p>

<p align="center"><a href="README.md">← Back to Triumph</a> · <a href="docs/tasks.md">Task log</a> · <a href="docs/research/README.md">Research</a> · <a href="docs/design/README.md">Designs</a> · <a href="docs/runs.md">Runs</a></p>

---

## Where we are (measured)

| | value |
|:--|:--|
| Frame | **7.9 fps · 127 ms** median, ~4,800 draws, ~345 dropped (AVX2 build, clean run) |
| Refused pipelines | **6** (was 311) |
| Shaders | **25,636 / 25,694** translate |
| The wall | our layer's **memory mirroring**: the title waits ~68 ms in present while our submit thread copies 72 MB up and 56 MB back |
| After the wall | the **GPU**: ~71 ms busy, of which ~26 ms is our own hashing and ~45 ms the title's. Up to ~40 ms of that may be BVH work, to be confirmed. |

## The goal, stated honestly

| target | verdict from the research |
|:--|:--|
| **30 fps native** | reachable on this machine: zero-copy + pipelining + GPU-side cuts |
| **60 fps displayed** | plausible with DLSS Frame Generation on a real ~30 |
| **60 fps native** | a stretch: needs the title's GPU work roughly halved again |
| **120 fps** | not on this hardware |

## How we work

1. **One change per measurement**, reported as **fps · ms · draws · dropped**. A faster frame that draws less is not a win.
2. **Quiet machine:** no compilers, no OBS, the small window while the remote desktop runs. `frames.py --check-cpu` must pass, or the run doesn't count.
3. **Run-to-run noise is ~7 %**, so small effects are measured inside one run with `alt` switches (20 s on, 20 s off).
4. Every new mechanism lands **behind a switch, off by default**, and goes into the launcher only after a run beats the baseline at the same draw count.
5. **Agent results are claims** until checked on disk: diff read, tests run, the switch's default checked.

## Phases

### Phase 1: measure what we have *(now, hours)*

| | step | expected | status |
|:--:|:--|:--|:--:|
| 1.1 | `VICTORY_ASYNC_PRESENT=alt`: overlap the title's frame with our submit | 127 → ~85–95 ms (est.) | 🔄 clean rerun running |
| 1.2 | `VICTORY_GATHER_BY_COPY=alt` (already exists, never measured) | ~2–5 ms GPU | ⏳ |
| 1.3 | `VICTORY_BACK_POOL`, `VICTORY_BACK_FEWER_WAITS`, `VICTORY_BACK_REGUARD` (built) | 2–15 ms submit thread | ⏳ |
| 1.4 | PIX timing capture with `VICTORY_MARKERS=1`: GPU time per pass, **confirm the ~40 ms BVH lead** | decides phase 3's order | ⏳ |
| 1.5 | RenderDoc capture of the red blocks: texture or shader? | decides the fix | ⏳ |
| 1.6 | `VICTORY_SHADER_STUB=1` floor run, plus an `fsrQuality` 1–4 GPU sweep | how much of the GPU is pixels vs our structure | ⏳ |

### Phase 2: remove the memory wall *(days → weeks)*

| | step | expected | design | status |
|:--:|:--|:--|:--|:--:|
| 2.1 | **Zero-copy step 1:** no copy-back except an allowlist | −35 to −40 ms submit | [uma](docs/design/uma.md) | 🔄 agent building |
| 2.2 | Zero-copy step 2: drop the hash lists | −20 ms card | uma | ⏳ |
| 2.3 | Zero-copy step 3: section heaps | copies 23.5 → ~8 ms | uma | ⏳ |
| 2.4 | **Pipelining:** fence tickets → 3 buffers → 2 frames ahead | max(title, submit, card) instead of their sum | [pipelining](docs/design/pipelining.md) | ⏳ needs copy-back compare-and-swap |
| 2.5 | Zero-copy steps 4–7 | a few ms each | uma | ⏳ |

**Expected end of phase 2:** ~50–60 ms (17–20 fps), with the GPU as the wall.

### Phase 3: cut GPU work *(weeks)*

| | step | expected | design | status |
|:--:|:--|:--|:--|:--:|
| 3.1 | **BVH/RT emulation track B:** page lookup once per node, vector loads, early exits | the ~24 ms of traces → ~5–8 ms, *if* 1.4 confirms it | [raytracing](docs/design/raytracing.md) | ⏳ gated on 1.4 |
| 3.2 | Indirect overhead: per-copy gather, barrier merge, one-record clamp, dispatch windows | 6–10 ms | [indirect](docs/design/indirect-overhead.md) | ⏳ |
| 3.3 | `memory()` fault-store branch (built, off) and scalar work per lane | 2–9 ms | [native-60 research](docs/research/native-60-gpu.md) | 🔄 built |
| 3.4 | Native quad ops, folded lane indices, reduction ladders | a few ms | [per-lane](docs/design/per-lane.md) | ⏳ |
| 3.5 | Title settings: LOD, texture LOD, culling, sharpening | a few ms | [settings research](docs/research/settings.md) | ⏳ |

**Expected end of phase 3:** ~33 ms. **30 fps.**

### Phase 4: correctness, all the way *(parallel throughout)*

| | step | status |
|:--:|:--|:--:|
| 4.1 | The last 58 untranslated programs: 40 descriptor-by-control-flow, 18 others | 🔄 two agents |
| 4.2 | The last 6 wave64 shaders: multi-wave groups and wave-wide branches | 🔄 agent |
| 4.3 | Rendering bugs: the red blocks, format and sampling audit | 🔄 agent |
| 4.4 | Ray-tracing correctness: BVH intersection against the ISA, builder wiring | 🔄 agent |
| 4.5 | Textures: mip-level upload → GPU detile → DirectStorage straight to the card | ⏳ [gpu-textures](docs/design/gpu-textures.md) |
| 4.6 | Picture checks: black/flat/saturated-block detectors on every run | ⏳ [picture-checks](docs/design/picture-checks.md) |

### Phase 5: past 30 *(after stable 30)*

| | step | design |
|:--:|:--|:--|
| 5.1 | DLSS in place of the title's FSR, and a lower render scale | [dlss](docs/design/dlss.md) |
| 5.2 | RT track A: native DXR / RayQuery, only if traversal loops are few | raytracing |
| 5.3 | DLSS Frame Generation: 60 fps displayed | dlss |
| 5.4 | Debug menu: overlay, live switches, then teleport and spawning | [debug-menu](docs/design/debug-menu.md) |

### Throughput *(makes everything above faster)*

| | step | status |
|:--:|:--|:--:|
| T.1 | `frames.py --alt`: on/off phase split | ⏳ |
| T.2 | Unattended experiment harness: queue, guard, screenshots, auto rows | ⏳ [harness](docs/design/harness.md) |
| T.3 | Green test suite (4 failing + 2 flaky), and CI on every push | ⏳ |
| T.4 | Review agent for finished branches before merge | ⏳ |

## Decisions waiting on the owner

- **DLSS licensing:** ship as an optional, user-supplied plugin, or not at all. It's closed and this repo is GPL-3.0.
- **RT track A** (2–4 months): only after track B and the loop-shape count.

---

## Update log

| time | update |
|:--|:--|
| 2026-11-26 22:20 | Plan written from: runs 26–27 + A/B, the census, 6 research reports, 8 designs. Pipeline fix, AVX2/LTO, wave64 part 1, markers and capture tool merged into the test build (`fast-integ`). In-game: refused pipelines **7 → 6**. The first `ASYNC_PRESENT` A/B was invalid (OBS plus agent compiles); clean rerun running. |
