# Making RT fast on NVIDIA (RTX 4070 Ti)

Read-only research, 2026-11-26. Tree D:\victory-work\wt-bisect. Legend: [V] verified in files this session,
[P] public figure from memory (no web check done), [E] my estimate. Nothing run or edited.

## Verified facts
- [V] docs/fork-ray-tracing.md: title uses no RT API. Traversal is compute code (shared-memory stack, raw 40-bit
  loads). Static-world BVHs ship prebuilt in console format (64 B box4 nodes with fp16 bounds, 128 B fp32 box nodes,
  triangle nodes, instance nodes). XboxBvhBuilder: 52 builder programs refit dynamic structures each frame. Only the
  node test is hardware (image_bvh[64]_intersect_ray). Cost on the GPU is "unmeasured". Five shadow-mask programs do not translate.
- [V] intersect.rs: each node test does a title-memory directory lookup (two for 128 B nodes), reads 28 dwords
  (fp32 box), 16 (fp16 box) or 10 (triangle), then 4 slab tests plus a sorting network, or a watertight triangle test. Bit-matched to a Rust reference on 311 cases.
- [V] Tri nodes hold vertices inline (the test reads three vertices), so static geometry is recoverable from the node data.
- [V] research-native-paths.md measured the builder at about +40 ms card time in the dorm (13.3 ms CopyForBVHUpdate, 24 ms traces).
- Not verified: what the 64 title traversal loops do after each node result. I did not disassemble them.

## (a) Emulated vs RT cores
- [P] RDNA2 ray accelerator: 4 box tests or 1 triangle test per CU per clock. Series X (52 CU, 1.825 GHz) is about 380 G box/s, 95 G tri/s peak.
- [P] 4070 Ti: 60 SMs / 60 3rd-gen RT cores, about 40 TFLOPs fp32. NVIDIA publishes no box-test rate; treat RT cores as roughly comparable to or above the console's peak on traversal, plus a full hardware traversal loop with no shader involvement.
- [E] Emulated box node: about 150-250 ALU ops (4 slab tests, sort network, half-widening) plus 1 dependent directory load and 16-28 dword loads. ALU-only ceiling about 40e12 x 0.3 / 200 = 60 G nodes/s, but dependent-memory latency, 32-lane divergence on a stack walk and wave64-exact forms cut that to roughly 1-5 G/s. So the emulated path is likely 20-100x below the 4070 Ti's own RT peak and 100x below the console. Measure first: a microbench of intersect.rs over a real BVH gives nodes/s directly (cheap, an afternoon).
- The +40 ms already measured suggests traversal is not the only cost: builders and CopyForBVHUpdate are a third.

## (b) Intercepting builder inputs; RayQuery replacement
- Dynamic: the builder DLL gets DXR-shaped geometry descs. Hooking its entry points (named exports/vtable, no title patching) lets us also call BuildRaytracingAccelerationStructure into a shadow BLAS/TLAS keyed by the console result address. Feasible. Build cost on 4070 Ti is normal DXR cost; it runs in parallel with (not instead of) the console build unless the title never reads console nodes of those structures.
- Static: no descs exist. Option: a one-time GPU pass that walks each prebuilt console BVH, emits its triangles (inline vertices) into a vertex/index buffer, and builds a BLAS; instance nodes map to TLAS instances. [E] this is the part that makes it work at all.
- Traversal: a RayQuery swap needs the whole loop replaced as a unit: ray setup in, then hit t, primitive/instance id, barycentrics out. Unit replacement means recognizing the loop (stack init, pop, node test, push sorted children, leaf handling, closest-hit update, 65,535 step cap) in each of 59-64 programs by signature in the ISA/IR, then emitting TraceRay-like RayQuery code that writes the same registers the code after the loop expects. Risks: the loop mixes in title logic (per-ray flags, culling, alpha tests, LOD/instance masks, leaf payload decoding). A per-program recognizer is feasible only if the loops share one compiled template (likely, since one builder/traversal library; unverified). Also need a map from raw root address to TLAS handle (lookup table in a buffer), and a cache invalidated when the builder runs.
- Hybrid cheaper step: keep loops, replace only the node test (already done). That gains nothing from RT cores; only a loop-level swap does.

## (c) Others (ideas only)
- [P] vkd3d-proton implements DXR 1.0/1.1 on VK_KHR_acceleration_structure/ray_query: opaque AS in app buffers addressed by GPU VA, RayQuery maps to SPIR-V ray query. It never ingests foreign BVH formats; it rebuilds from descs. Same model as our dynamic path.
- [P] Mesa RADV builds its own BVH in the RDNA hardware node layout with compute and traverses with the same instructions; it also has a shader-emulated RT path for GPUs lacking it (slow, tens of times below hardware). RADV's public node structs are a handy cross-check for the console format.
- [P] Console emulators (Xenia, RPCS3, yuzu/Ryujinx): none have an RT path to borrow; titles with RT are skipped or have it disabled.
- [P] NVIDIA features that would help once on DXR: Shader Execution Reordering and Opacity Micromaps (DXR 1.2 / NVAPI).

## (d) Cheaper fallbacks
1. Keep RT off, improve VICTORY_AMBIENT (the existing stand-in). [E] 0 ms, low effort, no GI fidelity.
2. Title settings: not found in the tree. Check the title's config for RT quality, probe update rate, ray counts, shadow-ray enable, and resolution scale (half or quarter resolution gives 2-4x fewer rays, [E]). Shadow-only RT is the cheapest visible win and the five untranslated programs are shadow-mask ones.
3. Dispatch-level throttling: run probe/GI traces every N frames with a temporal accumulate we already see; scale trace dispatch dimensions in the layer (risky if the title assumes full coverage).
4. Better node path: (i) if the BVH heap lies in one contiguous host buffer, bind it raw and drop the per-node directory load; (ii) pre-widen fp16 box nodes once per build; (iii) walk with fewer registers and mask divergence by wave. [E] 1.5-3x on the emulated test, medium effort, bit-exactness testable by the existing 311-case suite.
5. Skip CopyForBVHUpdate/refits for objects that did not move. [E] up to 10 ms of the 13.3.

## (e) Risks
- Exactness: console sorting and tie order, rounding and edge-case results are assumed, not public. A DXR hit may disagree on ties, shared edges (watertightness) and any-hit order; leaks look like light bleed or shadow speckle. Closest-hit order is robust, so shadows are safest; GI noise tolerates small differences.
- Memory: DXR BLAS for static triangles roughly 1-2x the vertex data plus scratch; likely hundreds of MB on a 12 GB card [E]; double the console BVH memory if both run.
- Build cost per frame: DXR refit of dynamic BLAS plus a full TLAS rebuild each frame, typically 1-3 ms on this class [P-ish]; the console builder must still run if any program reads its nodes.
- Correctness of a missed hook silently returns wrong hits, as an earlier partial builder crashed the title.

## Ranking (gain / effort)
| # | Option | Gain | Effort | Risk |
|---|---|---|---|---|
| 1 | Microbench + profile the emulated test (do first) | informs all | 1-2 days | none |
| 2 | Title settings: shadow-only, lower RT res, fewer rays | [E] 2-4x less RT cost | days | low |
| 3 | Node-path layout (drop directory, pre-widen, skip refits) | [E] 1.5-3x | 1-2 weeks | low-med |
| 4 | Static BVH to triangles to DXR BLAS + dynamic hook + loop-as-unit RayQuery | [E] 10-50x on traversal | 2-4 months | high |
| 5 | Stay off, improve stand-in ambient | 0 ms | low | none |
