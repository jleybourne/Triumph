# Where the 3x goes, and how to recover it (read-only research, nothing run or edited)

Legend: [V] verified by me in code or in the committed logs and CSVs; [E] estimate with reasoning; [?] unknown, needs the measurement named.
Sources read: `wt-bisect` (lower.rs, bitcode/opt.rs, resource.rs, cardmeter.rs, docs/per-lane.md, docs/frame-budget.md, diagnostics/timing-run-295s-2026-11-25.txt), `D:\triumph\victory-work` (wtv-main/CLAUDE.md, tools/census_default.csv, census-ranked.csv, arith-cost.md, math-cost.md, research-60-120.md), xcl at `origin/HEAD` (direct-dxil spec, dxil/reload.rs, compact.rs, lanes.rs, lower_round.rs). `docs/shader-cost.md` does not exist on main or in this tree.

## 1. The 3x is probably not one thing, and may be less than 3x

### What the card time is made of [V] (diagnostics run 280, 295 s, 4,600 draws)
- "All other lists" (the title's own) = 46.5-48.0 ms in ~215 lists. That matches the 45 ms figure. The same card timeline also carries our own lists: send-and-hash 77-83 ms, compare 15.5 ms. That run is a diagnostic build with DRED and per-list timing, so those numbers are inflated, but they are real card time and have to go to zero for 60 fps (the UMA plan). They are not shader cost.
- By first word of command (per-command timing, 47-50 ms): `indirect` 28-31 ms in ~4,400-4,500 commands (about 6.6 us each); `dispatch` 10.7-11.4 ms in ~1,330; alias copies 2.6; `draw` 1.9; `mesh` 1.0.
- 1,284-1,300 distinct kinds. The 35 costliest kinds total only ~14 ms, none above 1.55 ms. The cost is a flat tail, not a few monsters.
- Per frame, the layer adds 885 full waits ("waits for every earlier shader write") and 1,708 indirect commands that each get a clamp dispatch of ours (979 of them dispatches). The indirect commands are where the time is.
- The ten largest programs by static instruction count (~49k each, compute) do not appear among the 35 costliest commands. Five of the six hashes I checked never show up in a by-command line. They are first bound at frame 1665. Static size is not card time, and hand-tuning the top ten by size would recover little.

### Census, re-aggregated from census_default.csv [V]
Of 4,641,843 instructions: arith 44.6%, math_call 12.1%, bitcast 11.9%, select 11.6%, extract 6.7%, phi 3.8%, blocks 3.2%, buffer_load 3.4%, handle plus annotate 2.0%, wave_op 0.4%, lds_load 731 and lds_store 283 (about 0), barrier 8, guard_select 0.6%.
- Bitcast, extract and most phis cost nothing in SASS. Real ALU is ~57% of the count, so the static count overstates the cost.
- LDS emulation, wave64-split exchanges and barriers are ruled out as a main cause in this census set: they have almost no instructions.
- Stage mix: pixel 2.26M instructions, "unknown" (probably fused stages) 0.95M, compute 0.80M, mesh 0.49M, vertex 0.14M. The top 100 programs carry 35.7%.

### Why 3x is a soft number [E, from public specs]
- 40 vs 12 TFLOPS is a fp32 FMA ratio. The census is integer-heavy (address carries, shifts). Ada runs int32 on half its lanes: ~10 Tops/s against Series X's ~6 Tops/s, so about 1.65x, not 3.3x.
- Memory bandwidth is ~504 GB/s against ~560 GB/s, about 0.9x. Anything bound by latency or bandwidth should not be much faster here than on the console.
- The 33 ms is a frame cap, not a measured GPU time. If the console GPU idles part of it, the real ratio is higher. [?]
- Native versus upscaled: the launcher sets `fsrQuality=3` and the title runs its own FSR. If the console shades at a reduced internal resolution, "native, no upscaling" asks us to shade several times the console's pixels, and that alone could be most of the 3x. The earlier "540p saved 4%" test is invalid (CLAUDE.md: the internal scale did not change, "GPU unchanged"). [?]

Reading: 60 fps asks for a ~2.2x speed-up over the console's budget on hardware that is about 1.65x (integer) to 0.9x (bandwidth) better. Translation efficiency alone is unlikely to deliver that; work reduction and the structural overheads below have to carry part of it.

## 2. Candidates

Shares are of the ~30 ms excess over a 15 ms target, all [E] unless marked.

| # | Candidate | Evidence | Share | Method | Effort / risk |
|---|---|---|---|---|---|
| A | Launch, drain and layer-added passes around ExecuteIndirect and dispatch | [V] 4,500 indirect + 1,330 dispatch + 885 full waits + 1,708 clamp passes per frame; 6.6 us average per indirect command; flat tail | 6-12 ms (20-40%) | Fewer clamp passes (INDIRECT_GATHER already gathers 1,521 and misses 728); sanitise arguments in one pre-pass per list; precise barrier tracking instead of full drains; batch small mesh draws | Weeks / medium. Plain layer work, no shader change |
| B | Native resolution versus the console's internal resolution | [?] see 1 | 0-15 ms | Cardmeter A/B of `fsrQuality` 1-4 (see 3.2); title settings knobs | Hours / low |
| C | Memory by address (`memory()`, lower.rs ~2820-2865, compute only) | [V] per global access: ~8 ALU for page and clamp, a dependent directory `rawBufferLoad`, then an unconditional `rawBufferStore` of the fault record to a UAV (all lanes store to the sink slot 0 when nothing is missing), a heap handle with a non-uniform index, and 3-4 ALU from `address_plus` | 2-5 ms. The 49k programs show arith 20.6k with buffer_load 1.8k, which fits ~11 ALU per access; [?] whether they use this path | (1) Put the fault store inside a real branch taken only by missing lanes (small, safe). (2) Wave-uniform addresses: one lookup via `waveReadLaneFirst`. (3) UMA with a reserved (tiled) buffer: unmapped tiles read zeros and drop writes, as `memory()` does now, so the directory load and fault store go away. Needs a fault-detection replacement and a check of the 32-bit raw offset (one view per high dword, which `hi & 0xFF` already supports). Unverified API assumptions | (1) days, (3) weeks / high |
| D | Scalar (SGPR) work executed per lane and per host wave | [V] no scalar datapath in DXIL; wave64 runs as two host waves, each repeating the SALU stream; RDNA issues SALU for free in parallel. The census does not split S from V [?] | 3-6 ms | Count S and V instructions of hot programs offline. Mark uniform values (`lane_uniform` exists only with `exchange`) so the driver can use its uniform datapath. Hoist repeated scalar loads across blocks, as xcl's `reload.rs` does with available-expressions analysis (our `LOADS` pass is within one block) | Days-weeks / low |
| E | Huge shaders: registers, occupancy, instruction cache | [E] 49k IR instructions is ~0.5-1 MB of 16-byte SASS, far past the instruction cache; pressure from SSA values and phis. Not [V] | 0-5 ms, mostly in a few programs | Nsight "No Instruction" stalls, registers per thread, occupancy | Measure first |
| F | Divergence (per-lane model) | [V] census 16.5% of instructions; old run: 90.8 to 75.8 ms ceiling. The two refusals (`5dc3b2598662`, `6f225b641c5b`) need the per-lane EXEC model | 3-7 ms realistic | docs/per-lane.md plan: EXEC as data, live-mask analysis | Weeks / medium (silent wrong reductions) |
| G | f16 and packed math | [V] widened to f32 with `legacyF16ToF32` and `legacyF32ToF16` (23 sites); `VICTORY_F16_NEAREST` adds rounding emulation. Count of f16 instructions in hot programs unknown [?] | 1-3 ms | Native 16-bit ops (SM 6.2) lower to HFMA2 and round to nearest, which also removes the emulation. Ada's fp16 rate equals fp32, so the gain is conversions and packing, not 2x | Days / medium (bit-exactness) |
| H | DPP, `ds_swizzle` and lane-index arithmetic | [V] `fn dpp` (lower.rs 6697): ~6-10 ALU of lane maths plus a lane read per DPP op; `quad_perm` could be `QuadReadLaneAt` with a constant | ~1 ms | Constant quad ops; recognise whole-wave reduction regions as wave ops (xcl spec, step 3) | Days / low |
| I | Typed versus raw loads, WIDE loads | [V] `WIDE` is off: a dwordx4 load becomes four `rawBufferLoad` calls. Run 277 had water black with it (a partly out-of-range wide load may return zeros for the whole call) | 0-2 ms | Widen only when the range is provably in view (aligned structured buffer with stride); guard otherwise | Days / medium |
| J | Descriptor indirection | [V] handle plus annotate is 2%, `REUSE` merges within dominators; heap-index clamp is per handle | <1 ms | Cross-block handle reuse | Low value |
| K | DXC flags; HLSL versus DXIL | [V] not applicable: DXC never runs at run time (only as a test oracle in diff_tests.rs). We already emit DXIL bitcode directly with our own passes (opt.rs). The optimiser that matters is the NVIDIA driver's | 0 | None | n/a |
| L | Offline re-optimisation, hand-tuned replacements keyed by hash | [V] flat tail; the largest by size are not the costliest | ~2-4 ms for ~35 hand-tuned programs at best [E] | Rank by cardmeter time times dispatch count, not by static size. A hash-keyed override table in the shader cache is simple, but hashes change with title updates, and a replacement derived from the console shader is a legal question | High effort per shader / medium |

xcl ideas worth borrowing (ideas only, GPL-2.0): cross-block scalar load CSE (`reload.rs`, from a case of one descriptor loaded 2,399 times); reduction-region recognition; per-lane structurization. Not worth borrowing for speed: the `compact` loop-over-a-table form, which shrinks code at a runtime cost.

## 3. Measurement plan (existing tools)

1. Baseline without per-list serialisation. Use `VICTORY_CARD_METER=1` with `VICTORY_CARD_METER_PIPELINES=1` for per-pipeline card ms every 256 frames. Run `VICTORY_GPU_TIMING` once for per-command times. Report fps, ms, draws and dropped together, after the load spikes.
2. Resolution sensitivity. Set `fsrQuality` 1, 2, 3 and 4 (and `VICTORY_DISPLAY_MODE`) with cardmeter: card ms per setting gives the pixel-bound share, and tells you whether "native" is the real gap.
3. Floor versus ALU. Add a stub switch that swaps each shader for a trivial one with the same signature (it does not exist; small), or read the 1-command kinds' durations from the timing run. The residual is launch, drain and layer cost (candidate A), the part no shader change can touch.
4. Translator quality. `VICTORY_SHADER_OPT=0`, default and 127 (WIDE), plus `VICTORY_WAVE_WIDE` and `VICTORY_WAVE_MASKED`. The shader cache key includes SHADER_OPT, so each starts a fresh translation. Same scene, same draw count.
5. Weighted census. Join `census_default.csv` to cardmeter pipeline time by hash. Add translate-time counters for accesses through `memory()` and for non-uniform handles (`nonuniform_handles` already exists in `Lowered`), and an S-versus-V instruction count. Answers C, D and G per hot program.
6. Driver view. Nsight Graphics GPU Trace on the 35 costliest pipelines: SM throughput, issue-slot use, stall reasons (No Instruction, Long Scoreboard), registers, occupancy. Answers E and the memory-latency question directly.
7. Console reference [?]. If any console GPU capture exists, get the real GPU time and internal resolution. Without it the "3x" has no known denominator.

Order: 2 and 3 first (hours, and they decide whether to work on shaders or on the layer), then 5, then 6.

## 4. Bottom line
- Verified: the card cost is a flat tail of thousands of small indirect and dispatch commands, and the layer adds clamp passes and full drains of its own. The census and LDS or wave64 exchange are not the main cost. DXC flags and HLSL-versus-DXIL do not apply. Static size does not predict card time.
- Estimated: A (structure) and B (native resolution) are the largest shares; C (memory emulation, with a cheap first fix) and D (scalar replication) next; F, G, H and I are each a few ms or less.
- No single item gets 30 ms back. Without lowering the pixel work or the number of indirect and dispatch commands, 60 fps native is a stretch.
