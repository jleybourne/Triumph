# Shader quality: closing the gap between translated and native shaders (design)

Design only. No cargo build and no game run. Tree read: `D:\triumph\victory-work\wt-bisect`,
`crates/victory-shader` (`emit/lower.rs`, `emit/mod.rs`, `emit/shader.rs`, `emit/bitcode/opt.rs`,
`emit/bitcode/census.rs`, `ir.rs`, `structure.rs`). Designs read: `card-budget.md`, `per-lane.md`;
research: `native-60-gpu.md`, `results-gap.md`; notes `census-targets.md`, `arith-cost.md`, `math-cost.md`.

Two new measurements were made for this document, both offline (no game):

- **Full census, today's translator.** `VICTORY_SHADER_CENSUS=<file> victory translate shaders.rpf`
  with the built `target\vn\diag\victory.exe` (27 Nov): **25,694 programs**, one row each, with the
  console instruction mix (`c_valu`, `c_sop`, `c_smem`...) beside the emitted DXIL kinds. Saved as
  `D:\victory-work\census\census-full-wt-bisect-vn.tsv`. The old `census_default.csv` covered 5.6 %.
- **What the card is actually given.** 500 shaders picked at random from the run's shader cache
  (`%LOCALAPPDATA%\victory\cache\shaders`, 32,744 entries, all written 26 Nov, so all of them
  shaders the title really loaded), disassembled with the SDK's `dxc -dumpbin` and counted by opcode
  and by pattern: **2.08 M DXIL instructions**. Scripts are in `D:\victory-work\tools\shader-quality\`
  (`census.py`, `ops.py`, `legacy.py`, `legacy2.py`, `perstage.py`).

Labels: **[M]** measured (by the two runs above or a committed log), **[C]** read in code,
**[E]** estimate with the reasoning given, **[?]** unknown until the named measurement.

---

## 0. Summary

- **One pattern dominates: the console's "legacy" multiply** (`0 × anything = 0`). Two thirds of
  all float multiplies the title's shaders do are `v_mul_legacy_f32` (99,789 of 151,725 `fmul` in
  the sample [M]). `Lower::legacy_mul` (lower.rs 4050) emits each as `fmul` + 2 `fcmp oeq 0` + `or`
  + `select`: **five instructions where a native shader has one.** That is **19 % of everything we
  hand the driver** [M], and ~19 % of the real (non-free) operations [E]. No other emulation pattern
  is above 3–4 %.
- The next ones are small: the signed-zero fix-up on every float max/min (~3 % of real operations),
  the reciprocal emitted as an exact IEEE divide (~3 % [?]), f16 math widened to f32 with
  conversions (~2 %), and dword-at-a-time raw loads (90 % of raw loads are one dword [M]).
- Things the brief suspected that the data rules out as card-time items: the fault store (337
  stores in 500 used shaders [M]), the 64-bit address carries (1,631 `zext` in 2.08 M [M]),
  non-uniform handles (250 of 16,795 [M]), guards (0.7 %), loop counters (0.3 %), wave64 exchanges
  (LDS and barriers ~0 %), cross-block redundancy (`opt.rs` already removes it).
- **Static expansion:** 2.35 DXIL instructions per console instruction over the whole archive
  [M]; after removing what costs nothing in machine code (bitcasts, extracts, most phis), about
  **1.75 real operations per console operation** [E]. The legacy multiply is ~0.35–0.45 of that 0.75
  excess, the scalar stream that RDNA runs on its own unit is most of the rest, and the scalar
  stream is not a translation defect: a native PC build of the same source pays it too.
- **What it is worth in card time:** only the part of the title's 46 ms that is bound by
  instruction issue moves. The flat tail of ~5,800 small indirect commands and dispatches is not
  shader quality (card-budget ranks 2–8 own it). With an issue-bound pool of 10–24 ms (midpoint
  15) [E], the whole list below is worth **~3–7 ms** (midpoint ~4.5), most of it from the legacy
  multiply. Shader quality is a real lever but **not** the whole 1.65×.
- **A general optimiser (DXC's or LLVM's passes) is not the lever.** The overheads are exact
  emulation of console float rules, which no semantics-preserving pass may remove, and the
  driver already runs GVN/LICM/DCE-class passes. Do one cheap experiment (E2) to close the
  question; build our own narrow passes instead.
- **First builds:** (1) instrument the patterns in the census and run a 1-hour card
  microbenchmark that says whether NVIDIA's compiler already folds any of them (it has a native
  legacy multiply, `FMUL.FMZ`, that DXIL cannot name); (2) `VICTORY_LEGACY_MUL` with
  `exact`/`finite`/`plain` tiers, scoped by hash to the top 40 pipelines; (3) the two small exact
  float fast paths, `VICTORY_FMINMAX=native` and `VICTORY_RCP=fast`.

---

## 1. What the numbers say

### 1.1 Console side (full archive, 25,694 programs) [M]

| | all | compute | pixel | mesh | vertex |
|---|---:|---:|---:|---:|---:|
| programs | 25,694 | 2,682 | 12,056 | 8,765 | 2,191 |
| console wave64 | 90 % | 43 % | 91 % | 100 % | 100 % |
| VALU share of console instructions | 70.5 % | 70.5 % | 72.0 % | 67.2 % | 72.7 % |
| SALU + scalar memory | 21.9 % | 22.9 % | 20.2 % | 24.6 % | 18.7 % |
| emitted DXIL per console instruction | 2.35 | 2.41 | 2.30 | 2.34 | 2.42 |
| emitted DXIL per console VALU | 3.33 | 3.42 | 3.20 | 3.47 | 3.33 |

The 37 hashes the run-34 cardmeter names (the first two pipelines of each costly list kind) look
the same: ratio 2.51, scalar/vector 0.29. The largest programs are compute at 120k DXIL
instructions (`ffd9788ba9fec033`, `1250dc0656b3c091`, `fb515ca95d694c18`, ratio 2.9).

### 1.2 What the driver is given (500 used shaders, 2.08 M instructions) [M]

| kind | share | costs a machine instruction? |
|---|---:|---|
| integer ALU (add, and, or, shifts, icmp...) | 22.5 % | yes; but 99.8k `or` + 12.8k `and` are float-emulation glue (below) |
| float ALU (fadd/fsub/fmul/fdiv/fcmp) | 21.3 % | yes; **163,533 `fcmp oeq x, 0.0`** are legacy-multiply tests |
| select | 12.5 % | yes; 99.8k legacy, 12.8k signed-zero |
| bitcast | 11.4 % | no |
| FMad (`dx.op.tertiary` 46) | 7.1 % | yes, one FFMA |
| extractvalue | 5.8 % | no |
| phi | 3.7 % | mostly no |
| branches | 2.9 % | yes |
| raw buffer loads | 2.4 % | yes; 47k of 52k read one dword |
| handles + annotate | 1.8 % | mostly no (uniform, hoisted) |
| clamps (`binary` 40, UMin) | 0.75 % | yes |
| f16 conversions (`legacyF16ToF32/F32ToF16`) | 0.72 % | yes, plus masking around them |
| reciprocal as divide (`fdiv 1.0, x`) | 0.48 % | yes, several each [?] |

Pattern counts in the same sample [M]:

| pattern (where it is made) | count | extra instructions each | extra total | share of emitted |
|---|---:|---:|---:|---:|
| legacy multiply (`legacy_mul`, lower.rs 4050) | 99,789 | 4 | 399k | **19.2 %** |
| signed-zero max/min (`max_min`, 3845) | 12,797 | ~5 (2 free bitcasts) | ~64k | 3.1 % |
| reciprocal as IEEE divide (`v_rcp_f32`, 8047) | 10,359 | driver-dependent [?] | — | — |
| unfused mul + add (`v_mac_f32`, `v_mad_f32`) | 7,783 | 1 | 8k | 0.4 % |
| legacy multiply feeding an add (`*_legacy_f32` mac/mad) | 3,189 | — | — | — |
| f16 conversions | 15,402 | — | — | ~2 % with masks |
| non-uniform heap handles | 250 | — | — | ~0 |
| fault stores (`rawBufferStore`) | 337 | — | — | ~0 |

Legacy-multiply share by stage: pixel and compute ~5–7 % of their instructions are legacy
selects, mesh ~5 %, vertex ~4 % [M, `perstage.py`; that count includes some non-legacy zero
selects]. It is everywhere, not in a few shaders.

### 1.3 From instruction counts to card ms (the model every estimate below uses) [E]

- Title lists: 46 ms (run 34 control). Of that, ~6–12 ms is our in-list work (clamp passes, full
  waits; card-budget §1), and the ~5,800 small indirect commands and dispatches a frame carry a
  launch-and-drain floor of a few µs each that no shader change touches.
- What is left and bound by instruction issue in the shaders is the pool **P = 10–24 ms,
  midpoint 15**. A change that removes a fraction *f* of the issued operations saves about
  *f* × P. Memory- or latency-bound passes save nothing from fewer ALU operations.
- "Real operations" = emitted DXIL minus bitcast, extract, ~80 % of phis, handles and annotates:
  ~75 % of the emitted count. On Ada the removed float-emulation glue (FSETP, PLOP3, FSEL, LOP3)
  runs on the half-rate ALU pipe, so where a pass is pipe-bound the gain is larger than *f*.
  I do not count that bonus.
- Static counts stand in for executed counts. The sample is weighted by "loaded in a run", not by
  card time. Both are the main error source; VICTORY_PIPE_TIME plus the per-pipeline census join
  (section 7) replaces them.

---

## 2. What our DXIL does that a native shader would not

### 2.1 Legacy multiply: five instructions for one (the main item)

[C] `v_mul_legacy_f32`, `v_mac_legacy_f32` and `v_mad_legacy_f32` all go through
`legacy_mul`: `m = a*b; z = (a == 0) | (b == 0); select(z, +0.0, m)`. The console runs one
instruction. A native PC build of the same source runs one `FMUL` (the console's compiler picked
the legacy form for ordinary multiplies under its fast-math rules; the source never asked for
D3D9 semantics). NVIDIA hardware has the same instruction (`FMUL.FMZ`, `FFMA.FMZ`, used for D3D9),
but DXIL has no way to name it. Whether the driver recognises our five-instruction pattern and
emits `FMUL.FMZ` is **[?] and decides this item**: microbenchmark E0 (section 6.1) answers it.

When legacy and IEEE differ: only when one operand is ±0 and the other is ±inf or NaN (IEEE gives
NaN, legacy gives +0), and in the sign of a zero result (IEEE gives −0 for −0 × positive, legacy
+0). The real case is the `pow` idiom `exp2(y * log2(x))` with `x = 0`, `y = 0`: `log2(0) = −inf`,
and the console's 1 becomes a NaN, which bloom and temporal passes spread across the screen.

Provenance of the operands of the 99,789 legacy multiplies (`legacy2.py`: follow each operand up
to six definitions; "inf-capable" = `exp`, `log`, `sqrt`, `rsqrt` or a divide on the way, "load" =
a buffer, constant, input, sample or phi with only finite arithmetic between) [M]:

| operands | count | share | what the `finite` tier emits |
|---|---:|---:|---|
| load / load | 45,191 | 45 % | `fmul` |
| inf-capable / load | 30,866 | 31 % | `fmul` + one test of the load side + `select` (3) |
| inf-capable / inf-capable | 18,073 | 18 % | the exact five |
| finite / load, finite / finite, finite / inf | 5,659 | 6 % | `fmul` (or 3 where one side is inf-capable) |

Three tiers, one switch, `VICTORY_LEGACY_MUL`:

- `exact` (default, today): five instructions.
- `finite`: assumes values the title **loads** (buffers, constants, inputs, samples, LDS) are
  finite, and tracks what can make an infinity or NaN inside the shader (`exp`, `log`, `sqrt`,
  `rsqrt`, divide, reciprocal, and anything computed from them). A legacy multiply whose operands
  are both "finite by that rule" becomes `fmul`; one with one inf-capable side keeps only the
  other side's zero test. Keeps the `pow` idiom exact. Differs from the console only for a loaded
  ±inf/NaN met by a zero, or a finite overflow to inf met by a zero later, and the sign of zero.
  Removes **~12 % of real operations** [M counts, E weighting].
- `plain`: `fmul` everywhere (`FMad` for the mac and mad forms, so they fuse). What a native shader
  does. Removes **~19 % of real operations**. Risk: NaN where the console has 0.

Estimate: `finite` **1.2–2.8 ms** (mid 1.8), `plain` **1.9–4.6 ms** (mid 2.9), both **[E]**,
and **0** if E0 shows the driver already makes `FMUL.FMZ` of the pattern.

Side finding [?, check the ISA document before acting]: on RDNA2 (gfx10.3) the encodings that
`gfx10/opcodes.rs` names `v_mac_legacy_f32` (VOP2 0x06) and `v_mad_legacy_f32` (VOP3 0x140) are,
to my reading of the RDNA2 ISA, the **fused** `V_FMAC_LEGACY_F32` and `V_FMA_LEGACY_F32`. The
lowering (8011–8029) does an unfused legacy multiply then an add. If so, this is a small precision
mismatch today, and the fix (legacy test around an `FMad`) is also cheaper.

### 2.2 Signed-zero max and min (~3 % of real operations)

[C] `max_min` (3845) wraps every `FMax`/`FMin` (and so every `v_max3`, `v_min3`, `v_med3`) with
`or`, `and`, `icmp`, `and|or`, `select` so that −0 orders below +0 as RDNA does. Native: one
`FMNMX`. If NVIDIA's `FMNMX` already orders zeros that way (likely for its IEEE-754-2019
`minimum/maximum`-style behaviour, but **[?]**), the wrapper is pure cost and dropping it is
**exact on this card**. A one-shader bit test on the card answers it (E0).
`VICTORY_FMINMAX=native`. **0.3–0.8 ms** [E].

### 2.3 Reciprocal as an exact divide

[C] `v_rcp_f32` becomes `fdiv 1.0, x` with no fast-math flags (lower.rs 8047: "the hardware
reciprocal is approximate (1 ULP); this divides"). The DXIL writer emits no fast-math flags at all
(`intersect.rs` header says so). A native shader's `rcp()` is `fdiv fast` and becomes one
`MUFU.RCP` (plus at most one refinement). How NVIDIA lowers a flagless divide is **[?]** (a
correctly-rounded divide is several instructions and a slow-path branch). The console's own
`v_rcp_f32` is 1 ULP, so `MUFU.RCP` is no less accurate than the console, only different in the
last bit. `VICTORY_RCP=fast` puts the `arcp` flag on that one `fdiv` (the bitcode writer needs
the optional fast-math-flags operand on `INST_BINOP`). **0.3–1.3 ms** [E, ?].

### 2.4 f16 math widened to f32 (~2 %)

[C] Every f16 VALU op is: per source a shift, a mask and `legacyF16ToF32`; the op in f32;
`legacyF32ToF16` (plus ~20 instructions more with `VICTORY_F16_NEAREST`, because NVIDIA's
conversion rounds toward zero); then mask, shift and or to place the half. Packed `v_pk_*` ops do
it twice. Native: one `HADD2`/`HFMA2` for a pair, and native f16 rounds to nearest-even, which
also makes `F16_NEAREST` unnecessary. The sample has 15,402 conversions (0.74 %), so f16 is
**not** heavy in this title. `VICTORY_F16_NATIVE=1` (SM 6.2 16-bit types: the bitcode writer
needs `half` and the `.f16` overloads, plus the 16-bit shader flag). **0.2–0.5 ms** [E]. Ada's
f16 rate equals its f32 rate, so the gain is the glue, not 2× math.

### 2.5 Raw loads one dword at a time

[C] `s_buffer_load_dwordx2..x16` from an SRV becomes one `rawBufferLoad` per dword unless
`WIDE` (`VICTORY_SHADER_OPT` bit 64) is on, and `WIDE` blacked the water in run 277 (a
four-dword load partly past a view's end read as zeros whole on NVIDIA). Even `WIDE` passes
alignment 4, so the driver cannot use a 16-byte load anyway. [M] 47k of 52k raw loads are one
dword. A native shader reads the same data from a constant buffer (uniform datapath, `ULDC`) or
with 16-byte loads. Two options: `VICTORY_LOAD_ALIGN=1` (one call per four dwords with
alignment 16 where the offset is a constant multiple of 16 from the view base, and the host pads
every raw view's size up to 16 bytes so the partial-tail case cannot zero a whole load), or later
**constant-bank promotion** (bind scalar-only SRV ranges as root CBVs; a host and root-signature
change). **0–1 ms** [?]: the gain is LSU issue and MIO pressure, which only Nsight shows.

### 2.6 The fault store

[C] `memory_at` (3297) stores a fault record for **every** lane of **every** access by address,
to the sink slot when nothing is missing: all lanes of all warps write the same dword, one L2
slice. `VICTORY_MEMORY_FAULT_BRANCH=1` (built, off) stores only from missing lanes. [M] only 337
`rawBufferStore` and 73 `urem` in 500 used shaders: accesses by address are rare. **0–0.5 ms**
[E]; it costs a run, not a build.

### 2.7 Scalar work, wave64 and the scalar unit

[M] 21.9 % of console instructions are scalar, 90 % of programs are wave64. RDNA issues the scalar
stream on its own unit beside the vector one; on the host every surviving scalar operation is an
issued instruction in each of the two 32-lane warps that stand in for one console wave. This is
**the architecture, not the translation**: a native PC build of the same source has the same
uniform arithmetic, and NVIDIA's compiler puts what it can prove uniform on its uniform datapath
(which still takes issue slots). What is ours: values loaded by scalar loads through raw SRV
loads come back in vector registers, so the driver may not see them as uniform. Option
`VICTORY_SCALAR_HINT=1`: `waveReadLaneFirst` on every SGPR value the lowering loads through
`rawBufferLoad` (an SGPR is uniform by construction), and `memory_smem` handles made uniform
(today `memory_at` asks for a non-uniform heap handle even for SGPR addresses). **0–2 ms** [?]:
measure the uniform-datapath share of the top pipelines in Nsight before building.

### 2.8 Masked lanes, guards, branches, phis, loop counters

- Guards: 790,411 guarded of 32.5 M vector instructions (2.4 %) [M]; `guard_select` 0.7 % of
  emitted. The per-lane design's stage 5 already says "do not build unmeasured". ~0.
- Branches: 795k per-lane and 361k uniform [M]; they are real `CondBr`s, so the card skips work
  when a warp agrees. The driver sees SCC-based branches as uniform when the condition comes from
  constants. Nothing to gain without dynamic data.
- Loop counters (`LOOP_LIMIT`, `Tag::Counter`): 0.3 % (compute 0.6 %). They also hide trip counts
  from the driver's unroller. `VICTORY_LOOP_COUNTER=proven` (no counter on loops whose exit
  condition is uniform and monotone) is small: **0–0.3 ms** [E].
- Phis: 3.7–4.1 %, mostly coalesced away. The "every VGPR write reads the old value" rule (ir.rs)
  shows up only as guard selects, already counted.

### 2.9 Address arithmetic and descriptors

- 64-bit carries (`address_plus`, `memory_smem`): `zext` of a carry is 1,631 in 2.08 M [M]. The
  `arith-cost.md` conclusion that carries dominate does not hold for the shaders the title loads.
  An `i64` add form would save ~2 instructions per carry: negligible.
- Heap handles are made per block and reused by `REUSE` across dominated blocks; clamps by UMin
  are 0.75 % [M]. Handle-index clamps could go once VIEW_LIMIT is known at bind time, ~0.5 %.

### 2.10 Register pressure and occupancy

Unknown from static data [?]. Suspects: f16 pairs held as two f32s, the old-value operand of
guarded writes, and the 120k-instruction compute programs. Nsight's registers-per-thread and
occupancy for the top 40 is the only source. No item is proposed until it is measured.

---

## 3. Should we run a real optimiser over our output?

Options considered:

| option | what it would remove | verdict |
|---|---|---|
| Emit HLSL and compile with DXC | nothing exact: DXC's front end applies HLSL's fast-math by default, which would also reorder console float operations; bit control is lost; a rewrite of `lower.rs` | **no** |
| Run DXC's LLVM passes on our DXIL (`IDxcOptimizer` in `dxcompiler.dll`, or `dxopt`): instcombine, GVN, LICM, simplifycfg, DCE, then DXIL clean-up and re-validation | cross-block redundancy and algebra the driver also removes | **experiment only (E2)**. The patterns in section 2 are not redundant, they are console semantics, so no semantics-preserving pass may remove them. Generic passes on DXIL can also introduce constructs the validator refuses (vectors, `llvm.*` intrinsics, odd integer widths). Licence is fine (DXC is NCSA/LLVM-style, GPL-3.0 compatible), but it adds a ~20 MB DLL to the translation path |
| LLVM `opt` on a re-targeted module | as above, worse fit (newer LLVM bitcode is not DXIL) | **no** |
| Our own narrow passes (`opt.rs`) | exactly the semantic patterns, with facts the driver cannot know (console float rules, what the title's data may contain, that SGPRs are uniform) | **yes** |

Our own passes, in value order:

1. **Float facts** (new pass `RELAX`, `opt.rs`): a forward "may be inf or NaN" analysis over the
   module (sources: `exp`, `log`, `sqrt`, `rsqrt`, divide; through float arithmetic, select and
   phi to a fixpoint; loads and saturate are finite) that rewrites tagged legacy multiplies by tier
   (2.1). Also the place for the signed-zero and reciprocal relaxations (2.2, 2.3).
2. **Uniformity facts from the console's register file** (2.7): mark SGPR-born loaded values
   uniform; uniform handles for scalar addresses.
3. **Constant folding of float compares** with a constant operand (`FOLD` leaves all float work
   alone; `fcmp oeq <const>, 0.0` is exact to fold). Small, but free.
4. Not worth it now: global value numbering beyond `REUSE`, loop restructuring, scalarisation
   (the driver does these).

**E2 (closes the question, a day):** translate the top 40, run them through `IDxcOptimizer` with
`-instcombine -gvn -licm -simplifycfg -dce` plus DXC's DXIL clean-up passes, validate, and time them
with VICTORY_PIPE_TIME. Under ~2 % per pipeline: drop it for good.

---

## 4. What other translation layers do (described, not named)

- **A D3D9–11-to-Vulkan layer** faces the same legacy multiply (D3D9's `0 × inf = 0`). It offers
  three modes per title: off (plain multiply), a fast form, and a strict form that tests both
  operands as we do. The fast form is the default; strict is turned on per title where a picture
  breaks. That is the model for `VICTORY_LEGACY_MUL`.
- **A D3D12-to-Vulkan layer and its DXIL-to-SPIR-V converter** rely on the driver's compiler for
  optimisation. Their own passes are narrow and semantic: structurisation, vectorising raw-buffer
  loads only where 16-byte alignment is provable, choosing typed or raw buffer access by vendor,
  carrying `precise`/no-contraction only where the source had it, native 16-bit where the source
  had minimum precision, and per-title workarounds keyed by shader hash.
- **Emulators that recompile the same GPU family's ISA** (GCN/RDNA to SPIR-V) keep SGPRs as values
  the host compiler can see are uniform (derived from push constants and uniform loads), do SSA,
  constant propagation and dead-code passes of their own, lower EXEC to structured control flow,
  and offer "accurate float" toggles where the default emits plain IEEE operations for the
  console's special forms.
- **A DXIL-to-other-IR converter built on LLVM** runs the standard LLVM optimisation pipeline after
  conversion, because its back end expects optimised IR. Ours (the NVIDIA driver) does not need
  that.
- **Hand-tuned or recompiled replacements by shader hash** for the costliest few shaders are common
  in emulators and ports. Here the cost is a flat tail, hashes change with title updates, and a
  replacement derived from the title's shader raises legal questions (native-60 L). Not proposed.

Common thread: the big wins in all of them come from **policy on exactness** (which console or
legacy rules to emulate bit for bit, chosen per title or per shader with picture checks), not from
a general optimiser.

---

## 5. Ranked changes

ms are title card ms per frame from the pool model in 1.3 [E]. "exact" means the output bits stay
the console's on this card (once E0 confirms where marked).

| rank | change | switch (cache key) | est. card ms | effort | risk | verify |
|---:|---|---|---:|---|---|---|
| 0 | **Instrument + card microbenchmarks E0**: census kinds for each pattern; a card test that times each pattern and checks `FMax(−0,+0)` bits | `VICTORY_SHADER_CENSUS` (exists); new `#[ignore]` test | 0 (decides 1–3) | 2–3 days | none | census columns; test output on the 4070 Ti |
| 1 | **Legacy multiply tiers**, scoped to the top 40 first | `VICTORY_LEGACY_MUL=exact\|finite\|plain`, `VICTORY_QUALITY_ONLY=<hashes>` | `finite` −1.2 to −2.8; `plain` −1.9 to −4.6 (0 if E0 shows `FMUL.FMZ`) | 4–6 days | `finite` low–medium; `plain` medium–high (NaN where the console has 0) | gate (§8); `legacy_mul` census column falls; per-pipeline ms; picture checks with NaN/Inf detection on targets |
| 2 | **Native max/min** | `VICTORY_FMINMAX=native` | −0.3 to −0.8 | 1 day | none if E0's bit test passes; else sign of zero only | `valu_tests` min/max/med3 cases incl. ±0 and NaN; per-pipeline ms |
| 3 | **Fast reciprocal** (`arcp` on the one `fdiv`) | `VICTORY_RCP=fast` | −0.3 to −1.3 [?] | 2 days (fast-math flags in the bitcode writer) | low: ≤1 ULP like the console, different last bit | `valu_tests` rcp within 1 ULP; validator; per-pipeline ms |
| 4 | **Fault-store branch** (built) | `VICTORY_MEMORY_FAULT_BRANCH=1` (exists) | 0 to −0.5 | none | low | fault-record counts unchanged; per-pipeline ms of compute users |
| 5 | **Uniformity hints from SGPRs** | `VICTORY_SCALAR_HINT=1` | 0 to −2 [?] | 1 week, after Nsight shows low uniform-datapath use | low | Nsight UR share; per-pipeline ms |
| 6 | **Native f16** | `VICTORY_F16_NATIVE=1` | −0.2 to −0.5 | 1–2 weeks | medium (writer, flags, validator; bits change toward the console's own rounding) | `valu_tests` f16 and packed cases bit-exact; `wave64_tests` f16 sums; picture checks |
| 7 | **Aligned wide raw loads** with padded views | `VICTORY_LOAD_ALIGN=1` (replaces `SHADER_OPT` bit 64) | 0 to −1 [?] | 1 week | medium (run 277's black water class) | water and skin screenshots; per-pipeline ms |
| 8 | **Constant-bank promotion** of scalar-only SRV reads | `VICTORY_CBV_PROMOTE=1` | 0 to −1.5 [?] | 2–3 weeks (translator + root signature + bind) | medium | as 7 |
| 9 | **No loop counter on proven loops** | `VICTORY_LOOP_COUNTER=proven` | 0 to −0.3 | 3 days | low–medium (a hang if the proof is wrong) | loop tests; TDR-free 600 s run |
| 10 | **Fold float compares of constants** | part of `VICTORY_SHADER_OPT` (`FOLD`) | ~0 | 1 day | none | census |
| E2 | DXC optimiser experiment on the top 40 | `VICTORY_DXC_OPT=<passes>` (diagnostics only) | 0 to −1 [?] | 1–2 days | validator refusals | per-pipeline ms; drop if <2 % |
| — | Guard-run coalescing, i64 carries, LDS/wave64 exchange work | — | ~0 | — | — | not built (data in 2.8, 2.9) |

**Sum:** ranks 1 (`finite`) + 2 + 3 + 4 ≈ **−2 to −5 ms** (mid ~3.2); with `plain` on the
pipelines whose pictures pass, **−3 to −7 ms** (mid ~4.5). In card-budget's table this replaces
the rank-15 line ("f16, wide loads, quad ops, −1 to −3 each") with a better-founded **−3 to −7**,
and keeps rank 10 (scalar replication) at its low end: most of that is architecture.

---

## 6. The first three builds

### 6.1 Build 1: instrument and measure (rank 0)

1. **Tags.** `bitcode::Tag` gains `Legacy`, `SignedZero`, `Rcp`, `F16`. Tag the final `select`
   in `legacy_mul` (lower.rs 4050), in `max_min` (3845) and `med3` (3979), the `fdiv` of
   `v_rcp_f32` (8047), and every `legacyF16ToF32/F32ToF16` call. `census.rs` `KINDS` gains
   `legacy_mul`, `signed_zero`, `rcp`, `f16_conv` (counted like `guard_select`, beside their base
   kind). `apps/victory/src/translate.rs::census_line` needs no change (it prints `KINDS`).
   Output is byte-identical (tags are not written).
2. **Card microbenchmark** (`emit/cost_tests.rs`, `#[ignore]`, run on the 4070 Ti like the diff
   tests): compute modules built with our own `Module` API, 1 M threads, a dependent chain of
   4,096 operations of one form, timed with a timestamp pair (host.rs's runner plus a query heap;
   small). Forms: `fmul`; the legacy five; legacy `finite` three; `FMax`; `max_min`; `fdiv 1, x`
   flagless; `fdiv arcp`; f16 via conversions. Two chains in parallel so both pipes can issue.
   Output: ns per operation per form. If legacy-five ≈ `fmul`, the driver already makes
   `FMUL.FMZ`: drop rank 1. Same reading for max/min and rcp.
3. **Bit test**: `FMax(−0, +0)`, `FMax(+0, −0)`, `FMin` both ways, through a flagless DXIL
   `binary` 35/36, on the card (extend `valu_tests` the way the existing on-card cases run).
   If they order zeros as RDNA does, rank 2 is exact on this card.
4. **Census join**: `census.py hot40.txt` (section 7) prints per pipeline the four new columns and
   the predicted ms per item.

Deliverable: go/no-go per rank 1–3 in one page, before any of them is built.

### 6.2 Build 2: `VICTORY_LEGACY_MUL` (rank 1)

- **Where**: lowering keeps emitting the exact form, tagged (6.1). A new `opt.rs` pass `RELAX`
  (bit 128 of the pass set, on by default, a no-op when every relax switch is `exact`) runs
  **before** `DEAD`/`FOLD`, so the orphaned compares and `or`s die in the same run.
- **Analysis** (`RELAX`): one forward pass in dominator order plus a fixpoint over phis.
  `may_inf(v)` is true for `unary` 21/23/24/25 (`exp`, `log`, `sqrt`, `rsqrt`), `fdiv`, and any
  float operation, select, phi or bitcast-of-integer-op with a `may_inf` operand; false for
  constants, loads, inputs, samples, `unary` 7 (saturate), integer-to-float conversions and
  `legacyF16ToF32` of loaded data. (`plain` skips the analysis.)
- **Rewrite** of a tagged legacy `select(or(a==0, b==0), 0, a*b)`:
  `finite`: neither `may_inf` → `a*b`; one side `may_inf` → `select(other == 0, 0, a*b)`;
  both → unchanged. `plain` → `a*b`. The mac/mad legacy forms whose product feeds one `fadd`
  become `FMad` under `plain` (and under `finite` when both operands are finite).
- **Scope**: `VICTORY_QUALITY_ONLY=<16-hex hashes>` (parsed like `WaveSplitScope`,
  mod.rs 483). Unset means every program. A picture failure is bisected by halving the list.
- **Cache key**: `build.rs` collects every `"VICTORY_*"` literal in the crate into
  `VICTORY_SHADER_SWITCHES`, and `translation_fingerprint` (mod.rs 202) hashes their values, so
  both switches are cache-safe as long as they are read inside `victory-shader`. Changing the scope
  list re-keys every shader: warm with `translate --warm` before timing runs.
- **Tests**: `valu_tests` legacy cases (0 × inf, 0 × NaN, −0 × 5, NaN × 0) stay exact under
  `exact`; a unit test that `pow`-shaped code (`log` feeding a legacy multiply) keeps its guard
  under `finite`; the GPU diff tests with each tier; `opt.rs` unit tests for the rewrite.
- **Rollout**: `finite` on the top 40 → picture checks (NaN/Inf target scan from
  picture-checks.md §1) → `finite` everywhere → `plain` per pipeline where pictures pass.

### 6.3 Build 3: exact float fast paths (ranks 2 and 3)

- `VICTORY_FMINMAX=native`: `max_min` and `med3` return the bare `FMax`/`FMin` result (drop the
  zero fix-up; keep `med3`'s NaN rule). Only if E0's bit test passes; otherwise leave it in
  `exact` and offer it as a fourth relax tier.
- `VICTORY_RCP=fast`: the bitcode writer (`bitcode.rs` `bin`, ~440) gains an optional flags
  operand for float `BINOP` records (LLVM 3.7 fast-math bits: UnsafeAlgebra 1, NoNaNs 2,
  NoInfs 4, NoSignedZeros 8, AllowReciprocal 16) and `v_rcp_f32`/`v_rcp_iflag_f32` set only
  AllowReciprocal. Check the validator accepts it (DXC emits `fdiv fast`, so it should).
- Tests: `valu_tests` max/min/med3 with ±0, NaN; rcp of denormals, ±0, ±inf, within 1 ULP of the
  console reference.

---

## 7. Top 40 pipelines first

1. **List**: a VICTORY_PIPE_TIME run (control build, 600 s, steady state) writes per pipeline
   ms/frame and dispatch or draw counts. Keep the top 40 by ms as `hot40.txt` (hashes, stage, ms).
   Do not rank by static size: the largest programs are not the costliest (native-60 §1).
2. **Join**: `D:\victory-work\tools\shader-quality\census.py <census.tsv> <hashes>` already joins
   the census by the 16-hex hash (37 of 37 cardmeter hashes found). With Build 1's columns it
   prints per pipeline: legacy, signed-zero, rcp, f16 counts against its total, wave64,
   scalar/vector. Predicted saving per pipeline = ms × removed real operations / real operations
   × (issue-bound factor from Nsight; 0.5 until measured).
3. **Driver view** for the top 10: Nsight Graphics GPU Trace and shader profiler: issue-bound or
   memory-bound, `FMUL.FMZ` / `MUFU.RCP` / `FMNMX` present or not, registers, occupancy,
   uniform-datapath share. This turns the pool P into per-pipeline numbers.
4. **Scope**: every new switch obeys `VICTORY_QUALITY_ONLY`, so the first runs change only the 40.
5. **Read** the result on the per-pipeline ms of the 40 (sum and each) and on the cardmeter
   "all other lists" column, not on fps: results-gap.md §4 shows a frame-time test cannot resolve
   less than ~5 ms, and a card saving can turn into card idle while the submit thread is the wall.

---

## 8. Gate for every item (before a default changes)

1. Switch off: `translate` over the archive is byte-identical to the baseline.
2. Switch on: **census stays 100 %**: 26,084 translated, the same emitted counts and failure list
   as `census-fast-integ`, `--validate` 0 new dxil.dll failures, `--pipelines` 0 new refusals.
3. Census: the item's column falls as predicted; no program's real-operation count rises.
4. GPU tests: `emit/diff_tests.rs` (`translated_shaders_draw_as_pc_ones`,
   `translated_mesh_shaders_draw_as_pc_ones`, `translated_compute_matches_pc`), `valu_tests`,
   `wave64_tests`, with the switch set.
5. Picture checks (picture-checks.md): screenshot pairs at fixed spots (water, skin, sky,
   bloom, night lights); NaN/Inf scan of dumped targets for the float tiers.
6. Card: per-pipeline ms (VICTORY_PIPE_TIME) of the top 40 and cardmeter "all other lists", with
   `alt` cycles or same-build A/B runs; frame time last.

---

## 9. Open questions, in the order they unblock work

1. Does the driver already turn the legacy pattern into `FMUL.FMZ`? (E0 benchmark; Nsight SASS.)
   If yes, rank 1 is worth nothing and this document's total falls to ~1–3 ms.
2. Does `FMNMX` order −0 below +0? (E0 bit test.)
3. How does the driver lower a flagless `fdiv`? (E0 benchmark.)
4. What fraction of the title's 46 ms is issue-bound? (Nsight on the top 10; sets P.)
5. Are RDNA2's 0x06/0x140 the fused legacy forms? (ISA document; small correctness fix either way.)
