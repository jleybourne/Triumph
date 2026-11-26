# Ray tracing design: hybrid native DXR (A) and faster emulation (B)

Status: design only, nothing built or run. Tags: [V] read in source/docs this session, [X] inferred or unverified,
[E] estimate (no measurement behind it). Sources read: `docs/fork-ray-tracing.md`, `research-native-paths.md` item (4),
`crates/victory-shader/src/{bvh.rs,emit/intersect.rs}`, `crates/victory-gpu/src/pipeline.rs` (builder rule),
`crates/victory-hle/src/lib.rs`. `research-raytracing.md` did not exist when this was written; reconcile when it lands.

## Facts that shape both tracks

- [V] The "9 hle, 48 host" for `xboxbvhbuilder.dll` is the loader's report of that DLL's *imports*
  (`HleResolver::resolve`: registry first = hle, then `HostPolicy` = host; `victory-hle/src/lib.rs`, `BindReport` in
  `victory-loader/src/windows.rs`). It is not a table of the DLL's exports. So there is **no existing HLE seam on the
  builder's exports**; the DLL runs as title code in our process and its exports run as-is.
- [V] The DLL is a C++ class `XboxBvhBuilder(ID3D12Device11*, bool)` with DXR-shaped inputs and the console node format
  as output (native-paths note). Its export list/ABI is [X] (not enumerated yet): step 0 of Track A.
- [V] The builder's output is produced by ~52 compute programs the DLL records on a command list through our D3D12
  layer. The hook point we already own is therefore the D3D12 layer, not the DLL: we see every `Dispatch`, root view and
  root constant the builder makes (`pipeline.rs` `is_builder`, `VICTORY_BVH_BUILDER`, `QUARANTINE`, `HOLD`).
- [V] Node format: 64-byte nodes; kind in pointer low 3 bits (0-3 triangle k of a 4-triangle node, 4 box16, 5 box32
  (128 B), 6 instance, 7 procedural); triangle result = numerator, denominator, I, J numerators (hit t = num/den);
  I/J weigh the input triangle's 2nd/3rd vertex (`bvh.rs`). Traversal, stack, instance transforms, culling are program code.
- [V] Today: builder stays off in real runs (some pipelines refused: 64-lane waves on 32-lane GPU, or untranslated); 5
  shadow-mask trace programs don't translate; GPU cost unmeasured. [M, native-paths] ~+40 ms card time in one scene
  (13.3 ms CopyForBVHUpdate, 24 ms traces) with it on.
- [V] Static world BVHs ship pre-built in console format and are **never described to the builder**. DXR cannot ingest
  them. This is the central obstacle for A.

## Track A: Hybrid native RT (DXR BLAS/TLAS + inline RayQuery, console path as fallback)

Honest verdict up front: feasible only for **dynamic** (builder-built) geometry; static pre-built world needs either
(a) reverse-converting console nodes to DXR geometry on load, or (b) a mixed traversal. Neither is small. Recommend A
only if B's measurements show traversal is the bottleneck and the static world can be recovered (A3).

### A0. Discovery (no behaviour change) — switch `VICTORY_RT_TRACE=1` (log only)
1. Enumerate the DLL's exports (PE export dir via `victory-pe`) and log calls: arguments shape
   (geometry desc arrays, instance desc buffer address, build flags, scratch/dest addresses). Where the exports are
   C++ methods, hook by export-entry patch (we load the PE; per-build signatures are allowed by policy, per the native-paths
   note) or by IAT-style wrapper at the DLL boundary. [X] The method bodies may be inlined helpers; if the surface is not a
   clean export set, fall back to A1b.
2. Record, per build: kind (BLAS/TLAS), vertex/index buffer addresses, formats, strides, counts, transform, flags
   (dehydrated, update/refit, allow-compaction), instance desc array (transform, InstanceID, mask, SBT offset, BLAS
   address), and the destination/scratch addresses in title memory.

### A1. Mirror builds — switch `VICTORY_RT_MIRROR=1`
- A1a (export hook): new module `crates/victory-hle/src/modules/bvhbuilder.rs` (new) registers wrappers over the
  builder's exports: call original (console BVH still built, fallback intact), then enqueue a native build.
- A1b (layer hook, preferred if exports are opaque): in `victory-gpu` `draw.rs`/`pipeline.rs`, recognise the builder's
  stage sequence by program identity (existing `is_builder`) and read geometry from the *builder's input buffers* at the
  first stage, before translation. Fragile but doesn't depend on export ABI.
- Native build: new `crates/victory-gpu/src/rt.rs` (new): host `ID3D12Device5/GraphicsCommandList4`
  `BuildRaytracingAccelerationStructure`. Mapping key = title destination address of the console structure
  (the 40-bit/64-bit base the trace programs already use) -> native BLAS/TLAS `ID3D12Resource` + GPU VA.
  Refit builds (update flag) map to `PERFORM_UPDATE` on the same native BLAS (needs ALLOW_UPDATE; matches the
  title's per-frame refit, [E] similar cost class to the console refit, which we'd then also be running = double work in
  hybrid mode; see risks).
- Skinned/animated vertex buffers are written by earlier title compute, so build native structures **after** the same
  barriers: mirror at the point the DLL's own build is submitted, on the same queue/command list order.
- Geometry/instance ID mapping record (stored per native BLAS/TLAS): for each geometry index g: console triangle-node
  `primitive id` base (the dword the triangle node stores / the title's primitive numbering); vertex winding flip flag
  (see A4); per instance: InstanceID/mask/user data as the title's instance node holds them.

### A2. Recognise the traversal loop — switch `VICTORY_RT_REWRITE=1`
- Where: `crates/victory-shader/src/emit/` (new `rtloop.rs`), driven from the lowering of `image_bvh*_intersect_ray`
  in `emit/lower.rs`/`intersect.rs`. Needs the CFG after structurization (whatever pass currently emits loops; locate
  in `emit/`), not the raw instruction stream.
- Pattern (per the 64 programs; they share a codegen so a few shapes, [X] to confirm by dumping all 64):
  loop header -> pop node from LDS stack (per-thread slice) -> if kind in {4,5}: issue box intersect, push hit
  children nearest first -> if kind 0-3: issue triangle intersect, compare t = num/den to tmax, update closest, record
  (primitive, I, J) -> if kind 6: load instance transform, transform ray, switch descriptor base/root, push marker;
  pop-marker restores ray (TLAS->BLAS two-level) -> exit when stack empty.
  Recognition = dominance region around the instruction pair, with ray-setup (origin, dir, tmin/tmax, root pointer)
  as live-in values and (hit flag, t, primitive id, instance id, I, J, [front/back]) as live-out values; abort recognition
  (fall back, keep emulation) if the region has any side effect other than LDS stack traffic or live-out writes, or an
  unrecognised exit.
- Replacement: emit `RayQuery<RAY_FLAG_...>` (DXIL `AllocateRayQuery`, `RayQuery_TraceRayInline`, `Proceed` loop,
  `CommittedStatus`) with the TLAS bound as a root SRV (acceleration structure view) resolved from the root-pointer
  live-in via the A1 address->native map (a small per-dispatch table lookup in a constant buffer: console TLAS address
  -> native GPU VA). Needs shader model 6.5 / DXR 1.1 and `ID3D12Device5`; validate the module target in the DXIL
  emitter (it currently emits no RT-tier ops: a new capability flag in `victory-shader`).
- Live-out mapping:
  - hit distance: `CommittedRayT()` (already world-space t for the committed ray; match the program's convention: it
    computes t = num/den in the BLAS-local ray; if instance transforms are non-uniform scale the console's t is in
    object space while DXR's is `RayT` in world param units — for an unnormalised direction these agree only if the
    program scales the direction identically; verify per program).
  - primitive index: `CommittedPrimitiveIndex()` is the index within the geometry (D3D12), the console stores its own
    primitive id in the triangle node (`id` at byte 60 area, see `bvh_triangle`): map by building native geometry in the
    same primitive order and keeping the title's id <- geometry+primitive table (A1 record); GeometryIndex covers multi-geometry BLAS.
  - instance id: `CommittedInstanceID()` vs the title's own instance-node field; ensure TLAS InstanceID is set from
    the title's value in the instance descs recorded in A1.
  - barycentrics: DXR `CommittedTriangleBarycentrics()` gives (u,v) weighing vertices 1,2 of the *input* triangle, which
    is the same convention as the console's I/J (second and third input vertex, per `bvh.rs`) — [X] verify by the diff
    test; if not equal, swap. Do not forget the console returns numerators: the program divides by den.
  - front/back face and triangle-index-within-4 variants: derive from `CommittedTriangleFrontFace()`; the console's
    denominator sign encodes it (positive = clockwise from origin, left-handed).
- Keep fallback: the rewritten shader keeps the original emulated loop in the same function behind a spec constant /
  root constant `use_native` (per dispatch, set only when every TLAS the dispatch can reach has a native mirror).
  If any root pointer's mapping is absent (e.g. static pre-built structures) -> run the emulated loop. Granularity per
  dispatch, not per ray, to avoid divergence cost.

### A3. The static world (the hard part)
- Pre-built console BVHs in title data, never built via the DLL. Options:
  1. **Reverse-convert on load/first use** (switch `VICTORY_RT_STATIC=1`): when a trace dispatch first references a
     pre-built structure, a one-time GPU/CPU job walks the console nodes (we already have `bvh.rs` decoders:
     `children`, `boxes16/32`, `triangle_vertices`), extracts triangles (kinds 0-3), reconstructs vertex lists
     (dedup by position; the node stores positions, not indices; barycentric ordering from the vertex-map byte, `vertex_map`)
     and builds a native BLAS; instance nodes (kind 6) -> TLAS instances. Cost: [E] one-time hundreds of ms to seconds per
     large structure, memory ~ triangle count * 36 B + native BLAS (a few x). Procedural (kind 7) cannot map -> keep fallback.
  2. Mixed traversal: not realisable with RayQuery (can't enter a console node).
- Without A3, hybrid covers only dynamic geometry, and a ray that needs both worlds still has to go through emulation
  (two-level: the title's TLAS mixes them). Result: A is all-or-nothing per TLAS, which is why A3 is on the critical path.

### A4. Equivalence verification
- Reference = existing CPU `bvh.rs` + a CPU brute-force/BVH closest-hit on the same triangles. Compare per ray:
  hit/miss, t (relative tolerance 1e-5 or a few ulps; exact equality isn't expected: DXR is watertight-ish but
  implementation-defined), primitive id, instance id, I/J within 1e-4; **ignore** ties on edges/vertices
  (console: both triangles; DXR: one) by masking rays within epsilon of an edge in the generator or accepting either id.
- Real-game check: dispatch the same trace program twice (emulated, native) to two UAV outputs on the same frame
  (debug switch `VICTORY_RT_COMPARE=1`) and diff the output buffers on GPU, report % differing pixels/probes and
  histogram of |dt|; fail threshold e.g. >0.1% differing hits [E].

### A5. Cost/benefit and risks
- Benefit [E]: hardware RT cores on the 4070 Ti vs ~tens of ALU ops per node in emulation; 3-10x faster traversal is
  typical for RT cores vs software BVH walks, so the 24 ms trace cost [M] could drop to a few ms [E]. Also removes the need
  for the builder's per-frame GPU work *if* the console build can be turned off, which it cannot while the fallback exists:
  in hybrid mode we pay builder + native build [E +2-5 ms]. A pure-native mode (console build skipped, no fallback)
  would need *every* program rewritten, which the 64-program pattern may not allow.
- Risks: (1) static-world recovery fidelity; (2) a missed loop variant silently runs the emulated path (safe) or, worse,
  a wrong rewrite (unsafe: gate by A4 compare mode in CI of captured dispatches); (3) the builder's own writes into
  title memory that traces read directly (e.g. per-instance data, the title's own accessors reading node data outside the
  trace loop, shared with the irradiance-cache shading) still need the console build; (4) hook fragility (export ABI,
  per-build offsets); (5) D3D12 layer needs DXR tier 1.1 plumbing, new resource states (acceleration structure state,
  UAV barriers), build scratch sizing and compaction (not mirrored: skip compaction);
  (6) effort [E] 2-4 months with A3, ~4-6 weeks for dynamic-only proof. Matches native-paths' "months, very high" caution.
- Decision gate: do A0 (logging, 2-3 days [E]) and a one-program prototype of A2 against a synthetic scene before
  committing. Kill if the 64 programs have >5 distinct loop shapes.

## Track B: Faster emulation (the default recommendation)

Switches default off, each independently measurable. All in `crates/victory-shader` unless noted.

### B0. Measure first (prerequisite; `profile.rs` in victory-gpu already has per-role reports)
- Per trace program: ray count, nodes visited per ray (add an LDS/UAV counter behind `VICTORY_RT_STATS=1`), box vs
  triangle ratio, ALU vs memory time (PIX/Nsight). Every estimate below is [E] until this exists.

### B1. Node test cost (`emit/intersect.rs`: `bvh_boxes`, `bvh_triangle`, `bvh_node_words`)
- Today each instruction call loads node dwords through the page directory via `bvh_memory` (directory lookup per
  access) — the likely hot spot [E: 30-60% of emulated time]. Options:
  1. Hoist the directory lookup: one lookup per 64 B node (a 64 KiB page never straddles a node: nodes are 64-byte aligned;
     32-bit box128 nodes are also aligned), then use 4-16 B vector loads (`Load4`) instead of 28 scalar dword loads
     (`bvh_node_words(at, None, 16, 24)` = 24 separate words for box32). [E 1.3-1.8x on box tests]
  2. Cache the last page handle per thread/wave (consecutive nodes of a BLAS are adjacent; hit rate high).
  3. Optional: on node kinds, load children (16 B) first and test only valid children (`0xffffffff` skip) — avoids
     boxes for absent children; box16 half-to-float conversion via `f16tof32` intrinsics (check current emission).
- Triangle test: the watertight test is more ALU than Möller–Trumbore; keep it exact (bit-exact vs `bvh.rs` is the
  contract). Skip the division until a hit is established (return numerator/denominator, as the instruction does).

### B2. Early exits and culling (in the trace program rewrite, not the instruction)
- Shadow/any-hit style programs (the 5+ shadow-mask traces) only need occlusion: recognise `hit and t<tmax ->
  break` pattern and lift the early out. Many programs may already do this [X]; detect them by a dispatch-level stat.
- Ray-box test uses `tmax` of the current closest hit: confirm the program passes updated extents (program code, not ours).
- Ordered traversal: ensure BOX_SORT bit stays honoured (already) so nearest-first prunes earlier.

### B3. Wave-uniform traversal
- Programs written for 64-lane waves on 32-lane hardware (noted in `fork-ray-tracing.md`): any wave-wide stack sharing or
  `ballot` based "any lane still active" loops need exact forms (already done for correctness).
- Improve coherence: shared-memory stack is per thread (LDS pressure limits occupancy); use a short-stack or stack in
  registers for depth <=8 with LDS spill [E: occupancy +20-30%]. Wave-uniform loop exit: converge with `WaveActiveAnyTrue`
  so divergent threads don't re-enter the node-loading code with divergent addresses; sort rays by direction octant where the
  program already sorts [X].
- Wave-level node fetch: when all active lanes of a wave target the same node (common near the root), do a scalar-uniform
  load (`WaveReadLaneFirst` on the pointer if `WaveActiveAllEqual`) once instead of 32 identical loads. [E 10-25% on top-of-tree]

### B4. Node layout / storage (`crates/victory-gpu` title-memory host copy)
- Title memory is raw-addressed through a 64 KiB-page directory. Shadow BVH pages into a **dense native-layout copy**
  (e.g. box16 pre-expanded to f32, or plain SoA children) updated on a title-write detect (hard: the builder writes it
  on the GPU, the static data is read-only after load). Static world: convert once at load, no invalidation needed;
  dynamic: convert in a pass after the builder stage, [E +1 ms]. Benefit [E] removes the f16 conversion and directory lookup, 1.2-1.5x.
  Risk: the title may read/write node bytes outside the instruction (refit programs do), so only the trace-read path may use the shadow.
- Do not change what the title writes (it expects console bytes).

### B5. Use the title's RT settings at reduced resolution
- The title's RT settings are off. Turn on the cheapest sufficient set (shadows first: occlusion = early-exit B2; then
  reflections/GI probes) and run them at reduced trace resolution: either via the title's own scale options (if its
  settings expose them) or by dispatch-level override: scale `Dispatch` thread-group counts and the per-pixel
  index math is title code, so this is only safe through a title-visible setting, otherwise a wrong output. [X] Find the
  settings keys first (config/save file; no code change). Suggestion: half-res shadows/reflections + temporal accumulation
  already in the title [X]. Cost [E]: quarter ray count => 3-4x less trace time; leave the builder on only while an RT
  setting is on (it is gated by `VICTORY_BVH_BUILDER=1`).

### B6. Expected totals (all [E])
- B1+B3 together 1.5-2.5x on trace time; B4 another 1.2x; B5 3-4x on trace dispatches. Current 24 ms traces (+13 ms copy)
  could reach ~5-8 ms plus builder, i.e. likely playable on the 4070 Ti if the builder works. Track A would potentially beat
  that but at much higher cost/risk; B has no ceiling on correctness (bit-exact kept).

## Testing without the title's RT settings

Everything below runs as GPU diff tests in `victory-shader` tests (the existing harness runs emitted DXIL on the GPU over
311 ray/node cases against `bvh.rs`; extend it, do not invoke the title).

1. Synthetic console BVH builder in test code (`bvh.rs` test helper `build_box_tree` / `build_triangle_nodes`): write
   random triangle soups (1, 10, 1e4, 1e6), a two-level TLAS with 1-100 instances (rotation + non-uniform scale), BLAS in
   box16/box32 mixes, into a flat byte array registered as title memory (`Lower::bvh_memory` test stand-in already exists).
2. Ray sets: random, axis-aligned (zero direction components), grazing, rays through shared edges/vertices (document
   expected double-hit), origins inside boxes, tmin/tmax boundaries.
3. Reference oracles: (a) CPU `bvh.rs`-driven stack traversal + brute-force triangle min; (b) for Track A, a native DXR
   build of the *same triangles* through a small D3D12 test (needs the layer's DXR plumbing; or `wgpu`-free: direct
   `windows` crate in a `#[ignore]` GPU test).
4. Track B diff tests: each switch ON vs OFF produces identical dwords (bit-exact) over the same ray sets; counters
   verify the node-visit count decreased (B2/B3) and results match. Fuzz with proptest seeds; run in CI GPU lane only.
5. Track A diff tests: native RayQuery vs console emulation with edge-masking rules (A4); per-ID mapping test: assign
   random primitive ids and instance ids, assert returned ids equal the oracle's.
6. Trace-program level: a minimal hand-written compute "trace loop" in GCN-like test assembly (same loop shape the 64
   programs use: stack, push order, instance switch) so the A2 recogniser has a golden positive, a corpus of
   negative shapes (unknown exit, extra side effect) that must fall back, and the real 64 programs as dumps for a
   recogniser-coverage report (`VICTORY_RT_TRACE`), without running the title.
7. Performance micro-benchmarks on the synthetic scenes: nodes/s for emulated, B1-B4, and native; use this to calibrate every [E] above before any decision.

## Recommended order
1. B0 measurements + B1 (hoisted loads, vector loads) — days to a week [E], bit-exact, low risk.
2. A0 discovery logging in parallel (cheap, answers the export/ABI question and loop-shape count).
3. B3, B2, B4 as measurements justify. 4. B5 once the builder runs for real.
5. Go/no-go for A on A0 data and an emulation-vs-native microbenchmark on the synthetic scene.
