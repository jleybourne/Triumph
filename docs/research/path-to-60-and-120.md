# Research: from 7.9 fps to 60 and 120 (read-only; nothing run, nothing edited)

Legend: **[M]** measured (from CLAUDE.md, docs/, the RDNA3 reference fork findings), **[R]** read in code by me,
**[E]** my estimate (reasoning given; not a measurement).

## 1. What the measured numbers say about structure

- [M] Frame 127 ms. Render thread 6 ms, then ~68 ms waiting in present for our submit thread (74 ms
  at work: copies 23.5 ms/72 MB, copy-back 42.8 ms/56 MB). Card idle 44 ms, busy ~71 ms (26 ours,
  ~45 the title's, spread over ~93 kinds, none above ~7 ms).
- [M] 127 ms is not 74 ms. The two phases (title records/plays, then we prepare/submit) run one
  after the other. queue.rs docs for `present_behind` describe exactly this on another tree (77 ms in
  present, then ~57 ms recording the next frame while our thread has nothing to do). So the frame is
  roughly "title phase + submit phase", not their max. [E] On this tree the title phase is therefore
  about 127 - 74 = ~50 ms, which is itself 3x a 16.7 ms budget.
- [M] Card busy time is not pixel bound: dropping to 540p saved 4 % (runs.md row 6). Barriers are
  706/frame, 0.05 per draw. 211 lists, 4,500 draws.
- [M] The title is a console title; the RDNA3 reference fork says it "needs 33 ms" and menus here run at exactly
  33 ms / 30.0 fps, which looks like a frame limiter at 30 [E: inference, not verified].
- [E] The console budgets this GPU work in ~33 ms on a ~12 TF part; we spend ~45 ms on a ~40 TF part.
  That implies ~3x translation inefficiency on the card side. (per-lane.md says shaders that matter run
  4-8x the console's instruction count; the census says total divergence overhead is 16.5 %. I could
  not reconcile those two statements; whichever is right decides how much native wave/per-lane work pays.)

## 2. Candidates, ranked by (ms gained x confidence) / effort

| # | Candidate | Where | Attacks | Est. gain | Effort | Risk |
|---|---|---|---|---|---|---|
| 1 | **Measure `VICTORY_ASYNC_PRESENT=1`** (exists, `queue.rs present_behind`/`send_behind`, needs ASYNC_SUBMIT; not in launcher, not in runs.md) | queue.rs:70-140, 1314 | Serial title-phase + submit-phase. Present returns at once; title records frame N+1 while we submit frame N. | [E] 127 -> ~75-85 ms (frame becomes ~max(title phase, submit phase)). Biggest lever and already written. | hours (measure) | Med: assumes title memory is not rewritten before our thread copies it; the "all-on set" crashed, this switch was not in it, so untested alone. |
| 2 | **Fences that mean real GPU completion, then let the title run 2 frames ahead** (BUFFERS=2 -> 3, host.rs:8528; keep `bury_lag`) | host.rs presenter, queue.rs | Same serialisation; needed so UMA does not forbid pipelining (with in-place memory the GPU reads title memory live, so the title may only rewrite it after the fence). The title already does this on the console. | [E] enabler for #1 after UMA; 0 alone | days | High: lost writes, tearing of data |
| 3 | **UMA zero-copy** (planned) | shadow.rs, inplace.rs | Submit thread 74 -> ~15 ms [E]; card 71 -> ~45-50 ms [E] | ~55 ms thread, ~20 ms card | weeks | High. **Caveat I think the plan underweights:** a heap opened from title memory (`OpenExistingHeapFromAddress`) is host RAM; the GPU reads it over PCIe 4.0 x16 (~25 GB/s theory ceiling, vs ~500 GB/s VRAM) with high latency. Read-mostly large data (geometry, textures, big structured buffers) likely still wants a VRAM copy, so some of the 26 ms card lists (and the existing card-windows 2 GB / buffers 1.3 GB) may survive. Also address translation in shaders adds instructions to every memory op and could raise the 45 ms. |
| 4 | **Title settings.xml quality knobs** (docs/settings.md lists them; only `fsrQuality` tried): `asyncComputeEnabled`, `lodScale`, `grassLod`, `vehicleLodBias`, `vehicleHDCount`, `ssroSampleCount`, `waterReflectionResX/Y`, `volumetricsRaymarchResolutionUnclamped`, shadow*/rt* toggles, `maxTexUpgradesPerFrame`, `graphicsQualityPreset` | settings file, no code | The title's own ~45 ms GPU work and its CPU recording (fewer draws = fewer of the 36,700 calls). Only lever that removes work rather than making it cheaper. | [E] 5-20 ms of GPU in aggregate; unknown per knob. Cannot be larger than the diffuse 93 kinds allow. | hours each, one per run | Low, but draws drop, so every result must read `fps . ms . draws . dropped`; screenshot each. |
| 5 | **Frame limiter / vsync settings**: `desiredFPS`, `frameRateLimit`, `vSync` | settings file | If the title caps at 30, nothing above 30 can ever display; also makes present blocking. | 0 ms, but a gate for >30 | hours | Low; game logic may be tied to 30 Hz (unknown) |
| 6 | **Finish THIN (`thin-port` branch)**, then cut per-call cost | list.rs, list/thin.rs | 36,700 calls = 231 ms of CPU on title workers = **6.3 us/call** [M-derived]. A D3D12 call is normally well under 1 us. | the RDNA3 reference fork: 75 -> 68 ms [M, other machine]. After #1 this is the title-phase wall: 231 ms / ~10 free threads is ~23 ms; at 1.5 us/call [E] ~6 ms | days-weeks | Med |
| 7 | **Scheduling**: set `victory-submit` worker ABOVE_NORMAL/HIGHEST, MMCSS ("Games"), keep decode/pipeline workers below normal; leave timeBeginPeriod(1) (already in victory-hle runtime.rs). `keep_on_fast_cores` is a no-op on the homogeneous 10700KF [R]. Check HAGS on/off as an A/B. | queue.rs worker(), framelog.rs | Title pins ~31 threads to logical CPUs 0-13 [M, 14700KF]; on 8C/16T the submit thread competes. First look at the `usual frame` line: if submit-thread CPU time << at-work time, it is being preempted. | [E] 0-8 ms | hours | Low |
| 8 | **Present path**: BUFFERS 3, `ALLOW_TEARING` + `Present(0, ALLOW_TEARING)`, waitable swapchain with `SetMaximumFrameLatency(1)` (host.rs:9000-9030 uses FLIP_DISCARD, 2 buffers, `Present(0,0)`, no waitable object, no tearing flag [R]) | host.rs `presenter`, `present_now` | Latency and pacing, not throughput. The present is already a copy list to the back buffer (4K copy ~0.1-0.3 ms [E]). Needed to show 120 on a 60 Hz panel and to stop vsync from re-serialising a pipelined frame. | 0 throughput; -10 to -30 ms latency [E] | hours | Low |
| 9 | **Native wave/quad ops, per-lane branching, wave64-split** (planned) | victory-shader lower.rs | Card 45 ms. | census ceiling: 90.8 -> 75.8 ms total if all divergence overhead vanished = ~15 ms on the whole card [M]; [E] realistic 5-8 ms of the title's 45 | weeks | Med (silent wrong answers on reductions) |
| 10 | DLSS replacing FSR; lower render scale | planned | FSR passes at 4K, effective only for the pixel-bound part | [E] 2-5 ms given 540p saved only 4 % | days | Med (licence, motion vectors via 0x218) |
| 11 | Stop re-uploading our own copy-back writes (shadow.rs `copy_back_with` end) | shadow.rs:3185 | copy-back/copies | folded into UMA | hours | Low |
| 12 | `VICTORY_WINDOW_UPLOAD=1` (GPU upload heap/ReBAR; exists, off, unmeasured; `gpu_upload_buffer` host.rs:9210) | shadow.rs:482 | Removes the CPU-side "stop" copy for windows | [E] 0-3 ms; copies cost 5.5 us per op, ~1 us bytes [M], so bandwidth is not the issue | hours | Low |
| 13 | Barrier dedupe / enhanced barriers | draw.rs, host.rs (77 ResourceBarrier sites) | 706 barriers/frame | [M] other fork: "nothing to 0.1 ms each"; [E] 0-3 ms | days | Low value |
| 14 | ExecuteIndirect / bundles / work graphs / GPU-driven submission | indirect.rs, draw.rs:3551 | CPU draw cost ~4,800 draws | [E] <3 ms: draw submit is not the wall; bundles cannot hold per-frame state changes; work graphs would need the console command processor modelled | weeks | High, poor return |
| 15 | Pipeline library / `victory warm` | pipeline/library.rs, shader_cache.rs | Hitches only (270-660 s of a session over 200 ms [M, the RDNA3 reference fork]) | 0 median; fixes 1 % lows | days | Low |
| 16 | DLSS Frame Generation | planned | Displayed fps only | 2x displayed. RTX 4070 Ti has 2x FG, not multi-frame. | weeks | Med: adds latency; needs depth/motion vectors the FSR path already consumes |

Descriptors: the RDNA3 reference fork's biggest single gain (280 -> 124 ms, descriptor records left to the draws,
400,000 slots looked at, 40,000 mirrored) is a mechanism I did not confirm is in our descriptor/host.rs
(3,671 lines) and descriptor.rs; check before anything else on the submit thread. [unverified]

## 3. The arithmetic

Three independent resources must each fit the budget once frames are pipelined:

| Resource | Now | After planned + #1/#2/#4/#7 [E] | 60 fps needs | 120 needs |
|---|---|---|---|---|
| Title phase (game, render thread, recording on workers) | ~50 | 25-35 (THIN, fewer draws) | <=16.7 | <=8.3 |
| Submit thread | 74 | 10-20 (UMA) | <=16.7 | <=8.3 |
| Card busy | 71 | 40-50 (our 26 mostly gone, title's 45 less 5-15) | <=16.7 | <=8.3 |

- With pipelining the frame is the max of the three, not the sum: roughly 127 -> ~45 ms (22 fps)
  [E, optimistic].
- **The card is the wall.** To reach 16.7 ms the title's own GPU work must fall from ~45 ms to ~15 ms
  (3x). Everything in the plan on that axis sums to roughly 10-25 ms of removal [E]. The only
  reference point, a 4090 + 9950X3D, reached 40 ms with the copy model, i.e. about 2x this machine's
  GPU for a similar frame.
- **60 fps real**: not supported by any lever I can find. It requires (a) title GPU work cut ~3x
  (shader efficiency on the order of the console's, plus settings cuts), (b) the title phase below 16.7
  ms, which needs THIN and cheap per-call translation, and (c) the title's pacing not capped at 30
  and its simulation tolerating it. I would call it a stretch even with every item landed.
- **120 fps real**: no. 8.3 ms for a 4,800-draw, 93-kind frame the console budgets 33 ms for.
- **60 displayed**: reachable only through frame generation from a ~30 fps base, which needs
  card <= ~30 ms and the other two <= 33 ms. That is the plausible target: #1, #3, #4, #7, #9 and FG.
- **120 displayed** = 60 real base with 2x FG; same bar as 60 real.

Shortest honest path: do #1 (hours), then #4/#5/#7 (hours each), re-measure, and let the card
number from cardmeter, not the frame, decide whether UMA is worth weeks.
