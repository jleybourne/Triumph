# Per-lane model and native cross-lane ops: design

Read-only study. Nothing was built or run. Every file/line below was read in
`D:\victory-work\wt-bisect\crates\victory-shader\src` (same `docs/per-lane.md` as `main`;
`diff` is empty).

## 0. What the tree already is (this changes the brief)

`docs/per-lane.md` says every lane runs every path. The code says otherwise:

- `structure.rs` (decision 0003) turns EXEC and compare/carry masks into per-lane booleans
  (`Bools` BDD, `Atom::{Lane,Phi,Quad,Opaque,..}`). `Structured.guards[b][i]` is the extra
  per-lane condition on a vector instruction; `reach[b]` is what holds in every lane entering
  block b; `exits` are per-lane `LaneExit::Branch`.
- `Lower::exit` (lower.rs ~11038) emits those as real `Term::CondBr`. `begin_guarded/end_guarded`
  (4056) put an `if` around effects. VALU writes under a guard are `select(g,new,old)`
  (`write_placed`, tagged `Tag::Guard`, census kind `guard_select`).
- `wave.rs` already recognises reduction ladders: `sections()` finds `s_or_saveexec -1` /
  `s_nand_saveexec 0` .. restore, `simulate()` runs the DPP/permlane/readlane steps on symbolic
  lanes (`Sym::Red`), and `Lower::shape` lowers `Shape::Active/Prefix/ActiveF64` to
  `waveActiveOp` / `wavePrefixOp` / `waveActiveBit` over `select(on, x, identity)`. It matches
  semantics, not instruction text.
- `dpp` (6697) is the emulated path: waveGetLaneIndex + 6-14 scalar ops + one `waveReadLaneAt`.
  Native quad ops exist only for mesh (`quad_pick`, 4280; 4 reads + 4 cmp + 4 select).
- The "reference model" the doc wants (EXEC as i1, live-mask proofs, whole-wave branches) is
  `structure.rs` + `lane_regions`/`all_lane_blocks`/`lane_uniform` + `wave_wide`. It is built.
- `VICTORY_WAVE_MASKED` (named in the doc) does not exist in the tree; grep finds only
  `VICTORY_WAVE_WIDE`, `_LOCAL`, `_OWN_BITS`, `_DEEP_SKIPS`, `_WHY`, `VICTORY_FIRSTLANE_OWN`.

The 4-8x line traces to `D:\victory-work\perlane-design.md`, a summary of the *xcl* tree's
"naive path" ("current naive path executes every lane through every branch path (4-8x
inflation)"), copied into this tree's doc. No measurement of it exists here.

Why the two refusals really happen: not because DXIL cannot see off lanes. A wave op inside a
structured region runs over the host lanes in that region, and lanes outside have EXEC bit 0, so
with identity-element selects the result is exact (E, the console's active set, is a subset of
A, the host's active set; A minus E contributes identity). The refusal is only for a console
wave64 split into two host waves: combining the halves needs an LDS exchange plus group barrier
(`exchange_halves`, 3723), and a barrier must be reached by every thread, so it cannot sit in a
lane-branch region. Lines 1945/2043/3166/3812/7573/7654 are all that one condition.

## 1. Reconciling 4-8x with 16.5%

They measure different things and neither measures "executed instructions".

1. 16.5% is `wave_op+guard_select+select+phi` over the 1,433-program sample
   (`census-targets.md`: select 11.61%, phi 3.85%, guard_select 0.64%, wave_op 0.42%; 4,641,843
   instructions). It is a **static count of the emitted module** (`census_line` says "a static
   count of the whole program, not of one thread's path"), on 5.6% of 25,694 programs. It is also
   an **upper bound on divergence cost**, since `select` counts every v_cndmask/min/max/clamp,
   and `phi` counts every join. Honest divergence-specific share is guard_select+wave_op, ~1%.
2. The full-archive translate run (`census\translate-pipeline-samplers.txt`, 26,084 programs)
   gives: 791,463 guarded of 32,535,818 vector instructions = **2.4%**; branches 795,624 per lane
   (emitted as real branches) and 360,833 uniform. Static blow-up from guards is therefore tiny.
3. A static count cannot show "every lane runs every path": a guarded or divergent region has the
   same instruction count whether the hardware skips it or not. 4-8x can only be a dynamic,
   per-warp executed-instruction ratio, and only for code the tree does NOT branch around:
   (a) the retry forms for wave64 compute (`ir::without_early_outs`, `without_skips`, wave-wide,
   `with_local_waves*` in `mod.rs` ~370-560) which delete the console's `s_cbranch_execz` skips;
   (b) wave64 compute in 2 host waves with LDS+barrier exchanges (2 barriers per exchange);
   (c) loops running to the slowest lane (the console runs the same, but `LOOP_LIMIT` counters
   add `loop_counter` instructions).
4. So: both claims can be true if 4-8x is (a)-(c) on a few hot compute programs and 16.5% is the
   everything-else static average. The docs cannot settle it. Also, the 1,433-sample is not
   drawn-weighted, and `frame-budget.md` says the card side is only ~3x off the console budget
   by inference [E].

Measurement that would settle it (all cheap, no new emission):
- Per program, ratio `host total / c_total` already in `VICTORY_SHADER_CENSUS` output; rank by
  GPU time from a diagnostics run (draw counts by shader hash are in the run log; `translate
  --warm <hashes>` takes the same hash list) and take the weighted mean over the top 50 by
  cost. If the weighted ratio is near 1-1.3, 4-8x is dead; if some hot shaders sit at 4+, they
  are the retry-form ones.
- `VICTORY_WAVE_WHY=1 translate ...` prints `WAVE64-RETRY <form>: ...` lines per program: the
  count of programs landing in each of the non-exact forms is the population for (a)/(b).
- Dynamic truth: Nsight Graphics shader profiler (inst_executed per shader, per-warp) on the 20
  hottest shaders vs their static counts. That is the only number that is real.

## 2. Per-lane model (delta over what exists)

2.1 EXEC as data. Done (`Bools`, `guards`, `reach`, `phi_masks`). Delta: nothing structural.
Add a documented invariant instead, and test it (see Stage 4): "host lane executing point P
whenever its console EXEC bit at P is 1" (E subset A). This is what makes WaveActive* exact.

2.2 Live-mask analysis. `reach[b]` (path-sensitive, per block) and `all_lane_blocks` /
`lane_regions` (post-dominator based, lower.rs 341-410) are the live-lane facts. Missing facts
worth adding to `structure.rs`:
- `live_out_exec`: per block, the BDD of lanes that can still be active after it, to prove
  guards redundant across joins (a guard that equals `reach[b]` is already dropped; joins lose
  this on phi-merged masks). Only worth it if Stage 0 shows `guarded` is concentrated in few
  shaders. 2.4% static says low priority.
- `needs_all[b]`: blocks containing an exchange or a barrier (today a side effect via `need()`
  / `take_needs()`); make it an analysis input instead of an error-and-retry channel.

2.3 Structured branches that really skip. Already true for per-lane branches (CondBr, hardware
skips when no lane in the warp takes it) and wave-wide ones (`wave_wide`, `waveAnyTrue`).
What does not skip are the retry forms. Replace "delete the skip" with "predicated region":
for a `needs`-region, emit it as a flat, fully guarded block (blocks of the region reached
unconditionally, every instruction under `guards`, effects under `begin_guarded`), so every
thread reaches the barrier. This is if-conversion by region; it never changes values, only
executes dead lanes. Loops inside such a region keep the existing wave-any exit
(`structure_wide`). Implement as `structure::structure_flat(program, flat: &HashSet<usize>)`
beside `structure_wide`, selected by `lower::branches_around`'s result (same `needs`
mechanism, `take_needs`). It is what the two refusing programs (`5dc3b2598662`,
`6f225b641c5b`) want and is exact, unlike `local waves (APPROXIMATE)`.

2.4 Wave ops over inactive lanes, exactness rule. For any reduction R at block b:
- console result = op over lanes with `mask` true (the section's saved EXEC; `Sym::Red.mask`).
- host result = op over A of `select(on, x, identity)`; exact iff every lane with `on` true is
  in A (reaches b) and the identity is the op's identity (`Op::identity`; AddF accepts +0/-0,
  MinF uses +inf bits). Lanes that branched away have on=false by construction.
- Prefix ops: exact iff the lanes below a lane in A are all of the console's lanes below it
  that matter, i.e. off lanes contribute identity (same select).
- If wave64 split (exchange): the per-half result is combined through `exchange_halves`;
  requires `all_lanes(b)` today; after 2.3, true by construction.
- Residual hazard to test, not assume: Volta+ independent scheduling must have the active lanes
  converged at the wave op. Add a diff case with the reduction in a divergent branch and in a
  divergent loop exit.

## 3. Native D3D12 ops instead of emulation

Map for `Lower::dpp` (RDNA2, so no `row_bcast`; gfx9-only ctrl 0x142/0x143 are not decoded and
`dpp` returns "a DPP lane pattern" for them):

| console | now | native |
|---|---|---|
| quad perm ctrl 0xB1 (1,0,3,2) | `read_lane` + ~10 ops | `quadOp` dir X (opcode 123, dir 0) |
| ctrl 0x4E (2,3,0,1) | same | quadOp dir Y (1) |
| ctrl 0x1B (3,2,1,0) | same | quadOp dir Diagonal (2) |
| ctrl 0xE4 identity | same | no op |
| ctrl k*0x55 (broadcast lane k) | same | `quadReadLaneAt` (122) with constant k (1 call) |
| other quad perms | same | keep `read_lane`; for mesh `quad_pick` |
| row_shr/shl/ror (0x1xx), row_mirror/half_mirror (0x140/1), row_share/xmask (0x15x/0x16x) | `read_lane` | none native; xmask 1,2,3 = same quad ops; others stay `waveReadLaneAt` (it is one shuffle; cost is the index math) |
| DPP8 (`Dpp::Lanes`) in quads | read_lane | same quad-op mapping when selector is a quad perm |
| ds_swizzle quad mode | emulated | same quad ops (reuse the mapper) |
| `v_permlane16/x16` with constant selectors | 14 ops + read_lane | fold selector to a constant source-lane table: `lane ^ k` style gets a direct shuffle |

Constant lane-index folding: cache `waveGetLaneIndex`, `row = lane & ~15`, `pos = lane & 15`
once in the entry block (`lane_index` 4260 emits at `self.cur` each time; the REUSE pass only
dedups dominating copies); when `ctrl` is constant (always) and the pattern is lane +- n, emit
`select(pos >= n, WaveReadLaneAt(x, lane - n), bound_value)` and drop the dynamic masks.
`v_readlane` with `V::Const` lane: fold `cmp(cl, lane)`; in a compute wave64 split the holding
half is known statically, so only one half stores to LDS. Expect modest gain, because
`opt.rs` FOLD+REUSE already folds integer constants. Count after-opt instructions before building.

Quad op validity caveat: in compute, quad = lanes 4n..4n+3 of the wave only for layouts the SM
defines (1D groups / even-dimension groups). The tree relabels compute threads so host lane ==
console lane (`thread_id`, `console_lanes`), so use native quad ops first for pixel and mesh
(layout is the console's: comment at 6705), and for compute only after a compute diff case
passes on the card.

Reduction ladders: already handled for the saveexec form by `wave.rs`. Gaps to measure:
(a) ladders in a block where EXEC is already all lanes (no saveexec): `sections()` never starts
one; (b) sections with `Shape` missing, which fall to `section_inst`'s "a whole-wave section
step the simulation does not follow"; (c) shapes `Mixed`/`Red64F` only for float64 sum; (d) any
`WaveActiveAllTrue/AnyTrue/CountBits` forms done by ballot arithmetic instead of
`waveActiveBallot`+popcount (`v_mbcnt` handled at 8871).

### How to count each pattern (Stage 0, no emission change)

Extend `census_line` (`apps/victory/src/translate.rs` ~570), which already walks
`p.blocks.iter().flat_map(|b| &b.insts)`. Add columns, computed from the console IR:
- `c_dpp_quad`  : `Fields::Valu(v)` with `v.dpp` = `Row{ctrl: 0..=0xFF}`, split by canonical
  (0xB1/0x4E/0x1B/0xE4/k*0x55/other), and `Dpp::Lanes`.
- `c_dpp_row`   : other `Row` ctrls, by family (shr, shl, ror, mirror, share, xmask, unmodelled).
- `c_permlane`, `c_readlane` (constant vs computed lane), `c_writelane`, `c_mbcnt`,
  `c_swizzle_quad`, `c_swizzle_other`.
- `c_sections` and `c_dpp_in_section` using `emit::wave::sections` (it is `pub(crate)`; export a
  tiny `wave::census(p)` returning counts), so "dpp outside any section" = the emulated population;
  `c_section_unknown` = written values whose `Shape` is absent.
Then `VICTORY_SHADER_CENSUS=<file> victory translate <paths> --jobs N`, and a script that sums
columns, with weights from (i) program count, (ii) a hot-hash list from a run log. Ranking
the weight by shader cost is what decides which stage is worth building. Also record
`WAVE64-RETRY` outcome counts (`VICTORY_WAVE_WHY=1`) as the stage 4 population.
Go/no-go: build a stage only if its pattern is >=1% of vector instructions in the hot-weighted
set or sits in any shader of the top-20 GPU cost.

## 4. Staged plan (each behind a switch; default off)

Gate for every stage, before turning a default on:
1. `translate` over the archive with the switch off is byte-identical to baseline (fingerprint:
   `build.rs` auto-collects any `"VICTORY_*"` literal into the cache key, so switches stay
   cache-safe); with it on: emitted >= 25,636, same failure list, `--validate` (dxil.dll) 0 new
   validation failures, `--pipelines` 0 new pipeline refusals.
2. `emit/diff_tests.rs` GPU diff (needs `VICTORY_CONSOLE_DXC`, `VICTORY_PC_DXC`): the existing
   `translated_shaders_draw_as_pc_ones`, `translated_mesh_shaders_draw_as_pc_ones`,
   `translated_compute_matches_pc` with the switch set, plus the cases named per stage;
   `wave64_tests.rs` and `valu_tests.rs` (permlane, DPP8) with it set.
3. Instruction counts (census) do not rise on any program in the top-20 set.

**Stage 0: instrumentation only.** `translate.rs::census_line`, `wave.rs` (`census`),
`structure.rs` (nothing). Switch: existing `VICTORY_SHADER_CENSUS`. Output: the table in section
3 and the reconciliation numbers in section 1. Deliverable: go/no-go per stage.

**Stage 1: native quad ops. `VICTORY_NATIVE_QUAD=1`.** Change `Lower::dpp` (6697) to call new
`Lower::quad_native(x, ctrl)` (emits `quadOp`/`quadReadLaneAt`, `flags::WAVE_OPS`) when `quad`
and ctrl is canonical; keep the existing path for the rest. Same mapper in `ds` for
`ds_swizzle_b32` quad mode (`mesh_quad`, `quad_pick`): replace `quad_pick`'s 4-way select where
the selector is canonical. Pixel and mesh first; compute behind `VICTORY_NATIVE_QUAD=2`. New
diff cases: pixel "quad reads" (derive from the mesh one at diff_tests.rs ~611: X, Y, diagonal,
broadcast) and a compute one in `translated_compute_matches_pc`.

**Stage 2: lane-constant folding. `VICTORY_LANE_FOLD=1`.** `Lower::lane_index` (memoise lane,
row, pos in the entry block), `dpp` (row_shr/shl/ror/xmask with constant ctrl),
`valu_op` arms `v_permlane16_b32`/`v_permlanex16_b32` (7294) and `v_readlane_b32` (7545).
Verify with `valu_tests.rs::v_permlane16_and_x16_read_the_lanes_their_selectors_pick` and the
DPP8 test (1465). Only worth it if stage 0 shows the post-opt count is high.

**Stage 3: ladders the saveexec matcher misses. `VICTORY_LADDER=1`.** `wave::sections` (start
detection beyond saveexec: a block where `reach == TRUE` and a `v_cndmask`-identity feeds a DPP
chain), `wave::simulate`/`shapes`, `Lower::find_sections`, `section_inst`, `shape`.
Add `Shape` kinds only for what the count shows (e.g. all/any via ballot). Diff cases:
extend "wave operations" (diff_tests.rs 395) with WaveActiveAllTrue/AnyTrue and a prefix
product/min; the wave64_tests sum cases check the tree bit for bit.

**Stage 4: predicated regions for wave64 splits. `VICTORY_WAVE_FLAT=1`.** `structure.rs`:
`structure_flat` beside `structure_wide`; `lower.rs::branches_around`, `take_needs`, `exit`,
`all_lanes`; `emit/mod.rs::emit` retry ladder: try this form before `widen` and before every
APPROXIMATE local-waves form (exact before approximate is the existing rule). Target:
`5dc3b2598662`, `6f225b641c5b` and any program now taking the approximate forms. Tests: copy
`wave64_tests::a_wave_or_inside_a_lane_branch_needs_the_branch_wave_wide` with the flat form
(expect exact bits), plus the divergent-loop reduction case from 2.4.

**Stage 5 (only if stage 0 shows it): guard-run coalescing. `VICTORY_GUARD_RUNS=1`.**
`Lower::write_placed`, `begin_guarded/end_guarded`: consecutive VALU writes under one guard get
one `if`, replacing N selects with N phis (no static win, a skip when the warp is empty);
2.4% of vector instructions are guarded, so expect little. Do not build unmeasured.

## 5. Functions touched, in one place

lower.rs: `dpp` 6697, `quad_pick` 4280, `lane_index` 4260, `console_lane` 3697, `valu_op` arms
(`v_permlane*` 7294, `v_readlane_b32` 7545, `v_readfirstlane_b32` 7620), `shape` 1822,
`find_sections` 1758, `section_inst`, `exchange_halves` 3723, `wave_wide` 3821, `all_lanes`,
`exit` 11023, `branches_around` 418, `lane_regions` 355. wave.rs: `sections`, `simulate`,
`shapes`, `dpp_lane`. structure.rs: `structure_wide` (model for `structure_flat`). mod.rs:
`emit` retry ladder, `widen`. apps/victory translate.rs: `census_line`.

## 6. Risks

- Quad ops in compute (layout) and on NVIDIA with relabelled threads: gate by diff test, stage
  compute separately.
- Flat regions execute dead lanes: loads from garbage addresses. Reuse `Tag::Clamp` clamps and
  the existing `begin_guarded` for all effects; never flatten a region with an atomic that is
  not guarded.
- The doc's "4-8x" should not be quoted again until the weighted count above exists.
