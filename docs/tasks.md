<h1 align="center">✅ Task log</h1>

<p align="center"><i>What was done, what it measured, and what is next.</i></p>

<p align="center"><a href="../README.md">← Back to Triumph</a> · <a href="../PLAN.md">The plan</a> · <a href="research/README.md">Research</a> · <a href="design/README.md">Designs</a></p>

---

Every outcome is a run in [runs.md](runs.md), reported as **fps · ms · draws · dropped**. Two runs of
the same build differ by ~7 % (scene variance), so smaller gains count only when measured inside one
run (`alt` switches, split into phases with `frames.py --alt`).

*Last updated 2026-11-26 22:35.*

## Done

| | task | outcome | evidence |
|:--:|:--|:--|:--|
| ✅ | **Refused compute pipelines** | 311 → 7 refused (6 with the wave64 fix). The debug layer showed the shaders index the sampler heap, and an earlier "fix" had removed the flag allowing that. Draws 3,962 → ~4,800, dropped ~478 → ~345. | run 26 · `pipeline-samplers` `d1e1af34` |
| ✅ | **`fsrQuality=4` black screen** | Fixed by the same change; renders now, no gain yet (the GPU isn't the wall). | run 27 |
| ✅ | **Shader census** | 25,636 / 25,694 programs emit DXIL (99.77 %). The 58 left: 40 descriptor chosen by control flow, 10 hardware SGPR input, 5 descriptor read as data, 3 lane masks. | full-archive `victory translate` |
| ✅ | **Wave64 on 32-lane cards, part 1** | 1 of the 7 refused wave64 shaders runs at wave32; **confirmed in-game (refusals 7 → 6)**. | `wave64-split` `ace7a91f` |
| ✅ | **Decoder fix from the RDNA3 reference fork's PR #6** | Operandless VOP1 and the EXEC destination of VOP3 CMPX are decoded (ported with credit). | `3edc3e67` |
| ✅ | **AVX2 + LTO build** | 7.9 fps · 127.0 ms · 4,839 draws against 7.3 · 136.6 · 4,706. Inside run-to-run noise, so not proven. | run A |
| ✅ | **Copy-back piece-cut underflow** | Found by a new test: a copy starting mid-piece could skip a thread's range in release builds. Fixed. | `c8318f37` |
| ✅ | **GPU event markers** | Every draw and dispatch named by shader hash, our own work labelled (`VICTORY_MARKERS=1`). Review caught that names would show as single letters (ANSI flag); fixed. | `6b34f897` + `b0d0e6e1` |
| ✅ | **Capture trigger** | `VICTORY_CAPTURE_TOOL=renderdoc\|pix`, `VICTORY_CAPTURE_AT=<frame>` or F11. | `120ea8e4` |
| ✅ | **Copy-back thread pool and fewer card waits** | Built; review clean; await measurement. | `a8e38b30`, `7d68f32b` |
| ✅ | **`memory()` fault-store branch** | Only lanes with a missing page store the fault record. Review clean; awaits measurement. | `memory-fault` |
| ✅ | **Shader-stub floor switch** | Pixel and compute shaders emptied for a GPU-floor measurement. Review: thread count must match the real shader (fix in progress). | `55749403` |
| ✅ | **Copy-back write-watch rewatch** | Race-safe reset after our own write-back. Review: needs its own switch (fix in progress). | `dfe7d933` |
| ✅ | **`frames.py --alt`** | Splits an `alt` run into on/off phases with a per-cycle verdict. | `scripts/measure/frames.py` |
| ✅ | **Game folder renamed**, references fixed | Neutral path; config resolves. | local |
| ✅ | **Docs**: README, plan, tooling, credits, 15 research reports, 12 designs | Published. | `main` |
| ✅ | **Local backups** | Full git bundle (202 refs) verified; repeats every 30 min and after every run. | `D:\triumph\Backups` |
| ❌ | **Copy-back reguard for guarded memory** (`VICTORY_BACK_REGUARD`) | Inert: copy-back writes write-watched memory. Replaced by the rewatch above. | run B |

## Measured, awaiting confirmation

| | switch | result |
|:--:|:--|:--|
| 🟡 | **`VICTORY_ASYNC_PRESENT`** | **~11 % faster in-run**: on 144.1 ms against off 154.4 ms at the same draws (4,814 / 4,829), ratio 0.890 over 7 cycles. The machine was CPU-starved during the run without compilers (remote desktop suspected), so one quiet confirmation comes before the launcher. |

## In progress (agents)

| | task | state |
|:--:|:--|:--|
| 🔄 | **Zero-copy step 1**: no copy-back except an allowlist | built; slower so far (196.9 ms) — profiler named the cost, fixing |
| 🔄 | **40 "descriptor chosen by control flow"** refusals | building |
| 🔄 | **The other 18 refusals** + a shader-coverage progress page | building |
| 🔄 | **Wave64 multi-wave groups** (the 3 hardest) | building |
| 🔄 | **Rendering fixes**: the red blocks; format, sampling and BC-decode audit | building |
| 🔄 | **Ray-tracing correctness**: BVH intersection vs the ISA, builder wiring, RT refusals | building |
| 🔄 | **DLSS plugin, stage 1**: call trace, user-supplied DLL loader, draw hook | built; the 1440p→4K pass found (run 41); DLSS run next |
| 🔄 | **Pipelining stage 1 + copy-back compare-and-swap** | building |
| 🔄 | **Green test suite + CI** | 3 of 4 failing tests fixed (they were wrong tests); 4th in progress |
| 🔄 | **Review fixes**: stub thread count, rewatch switch | building |

## Next

| order | task | why |
|:--:|:--|:--|
| 1 | Quiet confirmation of `ASYNC_PRESENT`; find what starves the CPU during runs | first runtime win, and fair measurements |
| 2 | Measure: copy-back pool, fewer waits, rewatch, `GATHER_BY_COPY`, `memory()` branch | built and reviewed |
| 3 | PIX timing + RenderDoc red-block captures (markers on) | per-pass GPU time; texture or shader bug? |
| 4 | Stub floor run; reported 1080p output; `fsrQuality` sweep | how much of the GPU is pixels |
| 5 | Zero-copy step 1 measurement | the biggest expected win (−35 to −40 ms) |
| 6 | Pipelining stages 2–3 | overlap frames safely |
| 7 | RT track B, indirect-overhead stages, GPU textures | GPU-side cuts |
| 8 | DLSS test with a user-supplied DLL; Frame Generation; developer menu | after stable 30 |
