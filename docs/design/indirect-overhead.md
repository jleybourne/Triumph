# Indirect-command overhead: why the clamp passes and waits exist, and what can go

Read-only research; nothing run or edited. Tree: `D:\victory-work\wt-bisect\crates\victory-gpu\src`.
[V] = read in code or logs; [E] = my estimate, assumptions stated; [?] = needs a measurement.

## 1. Why they exist [V]

- Arguments of ExecuteIndirect come from title memory (host copies in `shadow`), written by the title's own compute passes. A wrong or untranslated pass leaves garbage; a draw of 4 billion vertices or a dispatch past the limits hangs the card until the driver resets (indirect.rs header, lines 1-30). So every execution reads a clamped copy, never the title's buffer.
- `indirect::bound` / `bound_ahead` -> `clamp_into` -> `record_clamp_as` (indirect.rs ~346-502) record, per command: UAV barrier + transition(scratch -> UAV), one compute dispatch (the HLSL at `SOURCE`, with a per-list budget of 2^27 charged by atomics), UAV barrier + transition(scratch -> INDIRECT_ARGUMENT), then ExecuteIndirect. That is two "wait for every earlier shader write" per pass, and a card trip from draw to compute and back (draw.rs comments: ~9 us each, run 243).
- Count buffers: in this scene there are none ("0 with a count buffer", "0 of more than one record"). ExecuteIndirect already receives the title's count buffer natively (draw.rs ~3564); the shader only reads it to bound threads. Nothing to win here.
- The 885 waits are a different thing: `list.rs::barrier_groups` (~1995) emits one global UAV barrier per title barrier group (851 groups, 0 buffer barriers). Our clamp waits (2 x 1,708) come on top and are counted separately (copystat.rs `IndirectBounded`, ~1292).

## 2. Where the 1,708 passes come from [V] (run 280 line: 1,708 passes, 1,113 read from a window, 1,521 gathered)

| Cause | Per frame | Why |
|---|---|---|
| indirect dispatches | 979 | `windowed` requires `kind != DISPATCH` (draw.rs ~3341); a dispatch may also write its own args |
| draws whose list "stopped gathering" | 728 (464 off the window's walk, 145 past window end, 115 no window) | `gather_ends` closes the whole list's gathering at any dispatch or storing draw (draw.rs ~2374; indirect.rs ~619) |
| first on list / stores | 5 | |

So the remaining passes are exactly those where something in the list may write the arguments.

## 3. Options

Per-pass cost used below: 5-9 us [E] (draw.rs comments say 9 us; 29 ms / 4,500 = 6.6 us average includes the execution itself).

| # | Option | Files / functions | Saves [E] | Effort | Risk | Switch |
|---|---|---|---|---|---|---|
| 0 | Measure the waits' share with the existing `VICTORY_BARRIER_TEST=indirect` / `groups` / `both` (diagnostics build, timing only, may race) | copystat.rs `barrier_test`; indirect.rs ~470 | ceiling for rows 3 and 4 | none | none (timing only) | exists |
| 1 | Per-copy gather tracking: a list stops gathering only for argument copies a shader was declared to write (`Shadow::gpu_writes`) | indirect.rs `gather_by_copy`; draw.rs `gather_ends`; host.rs `gather_wrote` (~7255) | most of the 728: 70% x 728 x 7 us = ~3.5 ms (2-5). Assumes the title's args writers are few buffers | zero to small (exists, off by default; needs a soak) | Hang-safe: gathered records are clamped when gathered. Failure is a stale record (wrong draw count), not a hang | `VICTORY_GATHER_BY_COPY=alt` (exists) |
| 2 | Batch dispatch clamps. Key fact: between two title barriers no pass may read what another writes, so args read in one barrier-free span were final at its opening barrier, and one pass can clamp all of them. Implement by letting `DISPATCH` into windows/gather under the row-1 per-copy rule, and adding a dispatch-aware "ahead" rule in the shader (kind 2 has none; only mesh does) | draw.rs ~3335-3400 (`windowed`); indirect.rs `SOURCE` kind-2 branch, `bound_ahead` | up to 979 x 7 us = 6.8 ms if all merge; the producer-barrier-consumer pattern probably merges 30-50%: 2-3 ms | Medium (about a week) | Hang-safe if every slot is still clamped and budgeted. Undrawn tail slots charge the budget (run 93 lesson): reuse the "asks too much -> empty" rule | `VICTORY_INDIRECT_WINDOW_DISPATCH=alt` (new) |
| 3 | Drop our first UAV barrier when the title's own barrier is the last thing recorded and no storing command since; dedupe back-to-back title barriers | list.rs `barrier_groups`; indirect.rs `record_clamp_as` (`first[]`); a dirty flag in `HostList` | up to ~880 barriers x 1-2 us = 1-2 ms; row 0 bounds it | Small | Low if the flag is conservative (copies, clears, storing draws, dispatches all set it) | `VICTORY_CLAMP_BARRIER_MERGE=alt` (new) |
| 4 | Narrow waits with enhanced barriers (`ID3D12GraphicsCommandList7::Barrier`): scratch UAV -> INDIRECT_ARGUMENT with SyncBefore COMPUTE_SHADING, SyncAfter EXECUTE_INDIRECT, instead of a global UAV barrier plus legacy transition. Legacy split barriers (BEGIN/END_ONLY) apply only to transitions and cannot hide a pass whose consumer is next | indirect.rs `uav_barrier`, `transition`, `record_clamp_as`, `record_gather_as` | 2-4 ms [?]: depends on whether NVIDIA drains less with sync scopes | Medium (needs List7 and layout-based state tracking of scratch) | Low | `VICTORY_CLAMP_BARRIERS=enhanced` (new) |
| 5 | One-record clamp kernel: no groupshared, no group syncs, no CAS loop (one saturating atomic add on the budget) when `records == 1` | indirect.rs `SOURCE`, second PSO in `clamp()` | 0.5-1.5 ms [E] | Small | Low (same bounds and budget); `warp_*` tests exist | `VICTORY_CLAMP_ONE=alt` (new) |
| 6 | Cut at barrier spans: record a span's clamps in a side list run before the span (extends row 2 to what windows cannot serve) | host.rs `prelude`, `record_gathers`, list `cuts()` | 1-3 ms, overlapping row 2 | High (weeks) | Medium (list-cut logic) | `VICTORY_CLAMP_SPANS` (new) |
| 7 | Drop clamps where ranges are "provably safe" | - | the rest, 3-6 ms | Medium | **High.** No proof exists: args come from translated shaders. Only as per-site trust with a canary: clamp the first N executions and 1 in 64 afterwards per (pipeline, args site); on the first difference clamp that site forever and trip `drvwatch`. One missed garbage record is a device removal | `VICTORY_INDIRECT_TRUST=0/1/alt` (last, opt-in, never default) |
| 8 | Clamp on an async compute queue | queue*.rs | none or negative | - | In-list clamps sit between producer and consumer in graphics order; only the one prelude per list (215 lists) could move, and a cross-queue fence costs more than the pass. Reject | - |
| 9 | Count buffers natively | draw.rs ~3564 | 0 (already native; none in this scene) | - | - | - |

## 4. Staged plan (each stage behind a switch, A/B inside one run)

Mechanism: reuse the existing `alt` pattern (20 s on, 20 s off, `EPOCH`/`LAST`, a log line at each change; draw.rs `jump_windows` and `gathers`, indirect.rs `gather_by_copy`). Factor it once into `fn alt_switch(name) -> bool`. Compare phases with `VICTORY_GPU_TIMING=1`: the "card's time by command" totals for `indirect` and `dispatch`, "indirect commands per frame by their clamp" (passes, gathered, "stopped gathering") and the copystat "waits for every earlier shader write" line. Add the phase (on/off) to those lines so the numbers split without guessing.

1. Stage 0 (no code): one run each of `VICTORY_BARRIER_TEST=indirect`, `groups`, `both`. Tells whether waits or passes are the cost.
2. Stage 1: `VICTORY_GATHER_BY_COPY=alt` in the launch set. Goal: "stopped gathering" 728 -> under 150; `indirect` -3 ms. Check stale records with `VICTORY_INDIRECT_WINDOW_CHECK=1`.
3. Stage 2: rows 3 and 5 (small, independent). Expect -1.5 to -3 ms combined.
4. Stage 3: row 2, dispatch windows. Expect -2 to -3 ms; dispatch passes 979 -> about 600. Extend the warp tests to kind 2 with garbage tails.
5. Stage 4: row 4, if stage 0 shows the waits are over half the pass cost.
6. Stage 5 only if still short: row 6; row 7 last, behind a canary.

Realistic total after stages 1-4: 6-10 ms of the 29 ms `indirect`, consistent with candidate A (6-12 ms) in `research-native-60-gpu.md`. No stage before 7 removes a clamp, so none can hang the card from unclamped arguments; stages 1-3 can at worst draw a stale, clamped record.

## 5. Caveats

- The ~9 us per pass comes from comments about draws (run 243); not seen measured for dispatch passes [?].
- The 885 title waits mirror the title's own barriers, which the console also pays; only dedupe (row 3) is justified, not removal.
- `bare_command_cost` (indirect.rs ~2494, ignored test) already measures a pass plus waits between draws on this card; run it to replace my per-pass assumptions.
