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
| 1.1 | `VICTORY_ASYNC_PRESENT`: overlap the title's frame with our submit | ~9–11 % | ✅ **in the launcher** (won in-run twice) |
| 1.2 | `VICTORY_GATHER_BY_COPY=alt` | ~2–5 ms GPU | ❌ neutral (0.995), rejected |
| 1.3 | `VICTORY_BACK_POOL`, `VICTORY_BACK_FEWER_WAITS`, `VICTORY_BACK_REGUARD` (built) | 2–15 ms submit thread | ⏳ |
| 1.4 | PIX timing capture with `VICTORY_MARKERS=1`: GPU time per pass | decides phase 3's order | ⏳ markers fixed (Unicode flag) |
| 1.5 | RenderDoc capture of the red blocks: texture or shader? | decides the fix | ⏳ |
| 1.6 | `VICTORY_SHADER_STUB=1` floor run, plus an `fsrQuality` 1–4 GPU sweep | how much of the GPU is pixels vs our structure | ⏳ |

### Phase 2: remove the memory wall *(days → weeks)*

| | step | expected | design | status |
|:--:|:--|:--|:--|:--:|
| 2.1 | **Zero-copy step 1:** no copy-back except an allowlist | −35 to −40 ms submit | [uma](docs/design/uma.md) | 🔄 copy-back 33.6 → 12.3 ms as designed, but frame 196.9 ms vs 138.0 control; profiler (run 42): 36 ms driver work recording per-range sync commands + 16 ms free-range update; both fixed (`f11a218f` scatter, `4bec9514` in-place compare, est. −8 to −9 ms for the latter), merged, not yet measured |
| 2.2 | Zero-copy step 2: drop the hash lists | −20 ms card | uma | ⏳ |
| 2.3 | Zero-copy step 3: section heaps | copies 23.5 → ~8 ms | uma | ⏳ |
| 2.4 | **Pipelining:** fence tickets → 3 buffers → 2 frames ahead | max(title, submit, card) instead of their sum | [pipelining](docs/design/pipelining.md) | 🔄 stage 1 + compare-and-swap built (`c1d18e91`), unmeasured |
| 2.5 | Zero-copy steps 4–7 | a few ms each | uma | ⏳ |

**Expected end of phase 2:** ~50–60 ms (17–20 fps), with the GPU as the wall.

### Phase 3: cut GPU work *(weeks)*

| | step | expected | design | status |
|:--:|:--|:--|:--|:--:|
| 3.1 | **BVH/RT emulation track B:** page lookup once per node, vector loads, early exits | the RT lighting passes are **dropped today** (builder off); turning them on adds ~40 ms, which track B cuts to ~15–20 ms | [raytracing](docs/design/raytracing.md) | ⏳ gated on 1.4 |
| 3.2 | Indirect overhead: per-copy gather, barrier merge, one-record clamp, dispatch windows | 6–10 ms | [indirect](docs/design/indirect-overhead.md) | ⏳ |
| 3.3 | `memory()` fault-store branch (built, off) and scalar work per lane | 2–9 ms | [native-60 research](docs/research/native-60-gpu.md) | 🔄 built |
| 3.4 | Native quad ops, folded lane indices, reduction ladders | a few ms | [per-lane](docs/design/per-lane.md) | ⏳ |
| 3.5 | Title settings: LOD, texture LOD, culling, sharpening | a few ms | [settings research](docs/research/settings.md) | ⏳ |

**Expected end of phase 3:** ~33 ms. **30 fps.**

### Phase 4: correctness, all the way *(parallel throughout)*

| | step | status |
|:--:|:--|:--:|
| 4.1 | The last 58 untranslated programs | ✅ **100 %: 25,694 / 25,694 programs translate** (own census on the merged build) |
| 4.2 | The 7 refused wave64 pipelines | ✅ 3 fixed, 🧪 3 to test in-game, ❌ 1 impossible exactly on 32-lane cards (needs all 32 KB of LDS) |
| 4.3 | Rendering bugs: the red blocks, format and sampling audit | 🔄 agent |
| 4.4 | Ray-tracing correctness: BVH intersection against the ISA, builder wiring | 🔄 agent |
| 4.5 | Textures: **raise the full texture-slot budget** ("card has no room", 167×), then mip-level upload → GPU detile → DirectStorage straight to the card | ⏳ [gpu-textures](docs/design/gpu-textures.md) |
| 4.6 | Picture checks: black/flat/saturated-block detectors on every run | ⏳ [picture-checks](docs/design/picture-checks.md) |
| 4.7 | **Release look:** settings profile one item per run (material mips, LOD 1.0, grass, shadows, water), then the RT builder for GI | ⏳ [fidelity](docs/research/fidelity.md) |

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
| 2026-11-26 22:40 | **`ASYNC_PRESENT`: ~11 % faster in-run** (0.890 over 7 cycles, same draws); the machine was still CPU-starved without compilers (remote desktop suspected), so a quiet confirmation comes first. **RT correction:** the ~40 ms BVH figure is from builder-on runs; today's frames skip the RT lighting passes instead, so RT is a correctness cost, not a saving. Review found and fixed: markers were ANSI-flagged (showed one letter), stub thread count, rewatch switch sharing. Game folder renamed to a neutral path. New: research on passthrough, RT, RT settings, presets, resolution, input, developer menu; designs for pipelining, indirect overhead, GPU textures, RT, harness, picture checks, developer menu v2. |
| 2026-11-26 22:45 | No runs this half-hour (agents building). **Fidelity finding:** ~336 of the ~339 dropped draws are the RT builder stage (off by default), not untranslated shaders; the dark look is missing GI; the texture-slot budget is full (new item 4.5). Built: pipelining stage 1 + copy-back CAS, rendering-fix view cast (no translator/BC bug found), review fixes. Test build `0e3d0578` compiling for the next measurement batch. |
| 2026-11-26 23:15 | **Shaders:** all 58 census refusals fixed across two branches (100 % expected once merged and re-counted); wave64: 3 of 7 fixed, 3 to test, 1 impossible. **Built and merged into the test build:** zero-copy step 1, UI-plane compositor, DLSS plugin stage 1 (NVIDIA-signed DLL ready), pipelining stage 1 + copy-back CAS, console profiles, RT correctness, green test suite. **Running:** zero-copy step 1 in-run A/B with UI-plane and upscaler logging. New designs: UI planes, saves, main menu, launcher; research: console techniques. |
| 2026-11-26 23:50 | Runs 28–31: ASYNC_PRESENT promoted to the launcher (baseline 7.3 fps · 136.4 ms · 4,960 draws); GATHER_BY_COPY rejected (neutral); zero-copy step 1 engaged only with the relay off (picture intact, ~600 MB skipped) and is now fixed to work with the relay (`uma-relay` 8a418757: two real data hazards found and fixed, command-processor keep narrowed); the copy-back trio crashed the title (REWATCH suspected, bisecting). Pipelining stages 2–3 committed. Owner granted autonomous work toward 30 fps. |
| 2026-11-27 00:10 | Runs 32–33: REWATCH confirmed as the crash; FEWER_WAITS+POOL no gain. **Zero-copy step 1 works with the relay** (copy-back −38 ms, uploads halved, no visual damage) but its owned-range comparison added ~48 ms to preparing, so the frame went 136 → 173 ms; that cost is the next fix. Zero-copy step 2 (drop the GPU hash lists) started. Shader branches reconciled; agent reports 25,694/25,694, own census pending. |
| 2026-11-27 00:45 | **Shader coverage 100 %** (25,694 / 25,694, verified). Control on the merged build: 7.2 fps · 138.0 ms · 4,729 draws. Zero-copy fully on: copy-back and card hashing fell as designed, but the submit thread spends ~100 ms more CPU in uninstrumented bookkeeping (196.9 ms); a profiler run comes next. Zero-copy step 2 and the read-fetch fix built. Batch of 6 runs in progress. |
| 2026-11-27 02:45 | Batch 34–39 summarised: no switch beat the control (7.2 fps · 138.0 ms); pipelining never reached 2 in flight because **the submit thread waits ~56 ms a frame on the card** (next frame reads bytes the shaders write) — fix in progress as GPU-ordered copies; the final 1440p→4K upscale is not a compute pass (DLSS hook extended to draws); wave64 forms work but must be scoped (`VICTORY_WAVE_SPLIT_ONLY=auto` built). The 'step 2' crash is a known intermittent title crash (11 earlier runs) — new stability item. Built: fork port 6, read fetch, 16-bit port (census still 100 %). Usage limit paused work 01:20–02:30. |
| 2026-11-27 03:15 | Runs 40–42: fork port 6 7.4 fps · 134.3 ms · 4,929 draws · 346 dropped (no crash; inside noise, not promoted); FSR draw trace 7.5 fps · 133.7 ms · 4,974 draws (found the 1440p→4K pass); zero-copy + profiler 4.9 fps · 202.1 ms · 4,743 draws (diagnostic) named the cost: per-range sync commands in the driver (36 ms) and the free-range update (16 ms). In progress: GPU-ordered copies, upstream port (64-thread card hashing, black-face texture fix). Next: DLSS run on the found pass. |
| 2026-11-27 03:45 | Run 43 (DLSS on the run-41 draw): 7.4 fps · 134.8 ms · 4,607 draws · 342 dropped, clean, DLSS not engaged: that draw is the loading-screen upscale; the world's upscale is `0309a446…`+`4f4ec7b1…` (run 44 in progress: hook finds it, but the loader picks the driver's NGX runtime and finds no evaluate entry point — loader fix next). Finished and merged into `fast-integ` (`dfb07a8b`, not yet compiled): upstream port (64-thread card hash alt, black-face texture fixes, translator fixes), wave64 scoped, 16-bit, read fetch, zero-copy fixes (scatter + in-place compare), GPU-ordered copies (`VICTORY_GPU_ORDERED_COPY`). Next: build, census, zero-copy and ordered copies vs control. |
| 2026-11-27 04:20 | Runs 44–46 on the merged build `2261b969` (census 100 %: 25,758/25,758): DLSS still not started (loader bug); **control 6.4 fps · 155.5 ms · 4,849 draws · 375 dropped — ~17 ms worse than run 34, suspected merge regression (page-write default), check queued**; zero-copy with its two new fixes **crashed (invalid)**, sent back. Card measured busy ~80 ms (34 ms our hashing), not 46. Research and designs added (coherency, native paths, GPU-driven, frame generation; card budget, submit split, recording cost, tracking cost); ~20 builders started on their top items, each behind an `alt` switch. Owner asked for parallel research/design/build overnight. |
| 2026-11-27 07:45 | Runs 47–52. **Wide card hashing won in-run (0.921; 121.3 vs 132.9 ms) and is promoted.** Ordered copies removed the 56 ms wait but were neutral overall (submit CPU +19 ms from per-fill steps; batched fix built). VIEW_KNOWN neutral, ABSENT_ONCE slower (not promoted). The intermittent crash is the title's own 30 s hang watchdog firing on our stalls (fix and stall diagnostic built). DLSS now runs through Streamline's MIT API (NGX's library can't go in a GPL program). Merged into `fast-integ`: present ticket, wide prepare, console-instruction fixes, zero-copy fixes, upload heaps (ReBAR confirmed on), steady presenter + dips log, pass-hash, guard fixes. Built and waiting to merge: batched ordered copies, GPU detile/static map, GPU-modified lists, port 5 A+C, crash fix, clamp/barriers. A 2.5 h usage pause stopped all builders (resumed). |
| 2026-11-27 08:10 | New measurement build `fast-integ` 00552a92: census 100 % (25,758/25,758); tests 358/359 (the one failure was a test-isolation leak, now fixed). It merges present ticket, wide prepare, console-instruction fixes, zero-copy fixes, upload heaps, steady presenter + dips log, pass-hash, guard fixes, send thread, crash fix, clamp/barriers. Run window 25–28 (present ticket, wide prepare, guard fixes, upload heaps; in-run A/B each) in progress. Also built: recording fixes (rebased), bind elision + ExecuteIndirect merging, card-authoritative stages 0–1. Batched ordered copies being integrated (top priority). No new measured results since 07:40. |
