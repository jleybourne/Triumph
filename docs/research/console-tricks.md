# Console tricks and the PC mapping (read-only; nothing run)

V = verified in a log, census or code; I = inferred. The title's internals are never claimed beyond what the names and counters show.

## Evidence base
- Census summary: 26,084 programs: 14,247 vertex/pixel, 2,682 compute, 8,765 mesh (34%). The file lists only duplicate and refused rows (853 names), so family counts below are from the summary or the sample, not a full table. Sample families: MS_VS_Transform*/MSNC (skinned) 494, GS_VoxelizeGeneric 196, PS_VoxelizeVisiblity 9, TexturedDeferred/Deferred* ~20, shadowmask:CS_TraceShadowRays_FullAdaptive 6, cutout_*:CS_RaytracingResolve 4, water_shading:CS_TransmissionVolumetrics 3, grass_batch_*:CS_BatchReadback 3, VfxVBuffer VS/PS ~50, Raytrace VBuffer and BVH Refit one each.
- Latest log (ending 20:49): ~4,720 draws and ~2,088 indirect commands per frame; indirect mesh dispatches up to 33-60k groups/frame; 143k DirectStorage writes (8.5 GB) into mapped pages; 8,811 placed host copies plus 45M placed-memory view records; usual frame ~136 ms. Zero hits for shading-rate, sampler feedback, ExecuteBundle, work graphs, hardware decompression.

## Ranked by expected PC gain

**1. Unified memory / zero-copy (V, biggest).** Evidence: copy-back 43-115 ms and mirror copies 24-86 ms per frame on the submit thread, plus "send-and-hash" and "compare" lists of ours on the card; 0.19% of stores are read by anything (design\uma.md). Today we shadow title memory and diff it. PC cost: dominates the frame (submit thread 96 ms of 136). Map: card-resident authoritative buffers on a default heap, with a small read-back allow-list (ocean textures, queries). Use GPU upload heaps (supported=true) for CPU-written streams and write-combined ReBAR windows (VICTORY_WINDOW_UPLOAD) rather than re-hashing. Already designed; this is the top item.

**2. GPU-driven rendering: ExecuteIndirect and our clamp passes (V).** Evidence: 2,088 indirect/frame, 960 clamp dispatches, 885 full waits, 7,273 records clamped (log; research-native-60-gpu says about 6.6 us/command). Console indirect args are trusted; we sanitise them. Map: native ExecuteIndirect with a count buffer and one sanitising pre-pass per list (design\indirect-overhead.md); precise UAV barriers instead of full drains (9-12 waits/frame shown). Work graphs are not a fit: the title's graph is fixed and the translated shaders are not node-shaped. I: 6-12 ms.

**3. Texture tiling, BCPack, DirectStorage (V).** Evidence: BCPack and zlib are unpacked on CPU workers (dstorage.rs); we then CPU-detile console swizzle (3-6 ms/frame refresh) and hash whole textures. Console GPU reads its tiled layout directly. Map: upload the tiled bytes, detile in a compute pass (design\gpu-textures.md), GPU BCn-sparing; consider Windows DirectStorage GDeflate transcoding only for load time (BCPack has no PC GPU decoder: I). Reserved resources (tiles.rs) already stand in for sparse placement.

**4. Mesh shaders (V partly).** Evidence: 8,765 mesh programs; DispatchMesh used natively (draw.rs 2712, host mesh-tier check). Already the right mapping on a 4070 Ti (mesh tier 1; run only with SM 6.6). Cost is the indirect-mesh arguments (max group count 60k) which hit the 65,535 clamp; no gain from rework except avoiding clamp passes (item 2). I: no pure win left.

**5. Async compute overlap (V that title uses a compute queue; I about the amount).** Evidence: our queue.rs models the title's compute queue waiting on the graphics queue's depth; our host runs on one DIRECT queue (host.rs 880). Result: overlap is lost, compute (3,030 programs, ~1,330 dispatches/frame per earlier runs) serialises with draws. Map: real D3D12 COMPUTE queue with fence waits matching the title's. Gain I: 2-5 ms once item 1 removes the CPU cap; today the card is idle 40-80 ms per frame, so no gain yet. Risk: hazards when our diff passes interleave.

**6. FSR plus dynamic resolution (V that scene is 2560x1440 to 3840x2160 present; the dynamic part is I).** Evidence: fsrQuality 3/4 same scene size; FFX console passes refused (device 0x218). Map: intercept the loader front door and run native DLSS or FSR PC (design\dlss.md), lowering internal scale. Card-side gain I: 2-5 ms; larger if native 4K shading is current (research-native-60-gpu B).

**7. Wave64 / DPP / LDS (V that wave64 programs exist; I on rest).** Evidence: "64-lane waves on a 32-lane GPU" refusals; 576 refused compute pipelines in the last log; census shows lds ops near zero and wave_op 0.4%. Map: wave size is fixed 32 on NVIDIA; use per-lane exchange lowering (design\per-lane.md); ForceWaveSize is unavailable. Real cost low per the census; the cost is refusals and 2x scalar repeat.

**8. Hardware BVH and RT GI (V present, off).** Evidence: shadowmask traces, cutout RaytracingResolve, Raytrace VBuffer, BVH Refit/Copy; "ray-tracing structure builder, which is off" drops ~340 pipelines/frame; ~40 ms card with it on. Static world BVHs ship pre-built (cannot feed DXR). Map: DXR BLAS/TLAS for dynamic builder inputs plus inline RayQuery; stay off meanwhile (research-raytracing.md). GI: GS_VoxelizeGeneric (196, all refused: address in SGPR) and PS_VoxelizeVisiblity imply voxel-visibility GI (I about use). Map: native geometry-stage voxelise using conservative raster; blocks on the refusals.

**9. Visibility buffer / deferred texturing (I).** Names only (VfxVBuffer, Deferred*, RaytraceVBuffer). No dedicated PC feature; PC gain none. Pure shader cost.

**10. Console-only D3D12.X (V).** Placed resources (8,811 placed copies; 94k view shapes) map to CreatePlacedResource via heaps with rank churn costs (1,319 re-keyed). Fast clears and DCC options are accepted and ignored; ESRAM does not exist on this class. Direct GPU address in shaders: 218k computed descriptor terms and "address in an SGPR" refusals map to bindless descriptor heaps plus root buffer views; the memory-by-address compute path (2-5 ms) can become raw descriptor-heap buffer access once item 1 gives stable GPU addresses (GPU VA).

**Not used (V): VRS, sampler feedback, work graphs, bundles.** Nothing to port. DLSS-FG or reflex are PC-only extras.

## Order of work
1 zero-copy; 2 indirect clamping/barriers; 3 GPU detile; 5 compute queue (after 1); 6 upscaler; 8 DXR for dynamic geometry.
