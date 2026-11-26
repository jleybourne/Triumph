# Native paths: what could bypass emulation instead of speeding it up

Read-only research, 2026-11-26. Tree: `D:\victory-work\wt-bisect`. Legend: **[V]** verified by me on disk this session,
**[M]** measured figure quoted from CLAUDE.md or docs (not re-measured), **[X]** read in xcl/cool-fox docs (their claim),
**[E]** my estimate, with reasoning. Nothing was run, built or edited.

## What I verified on disk

- **[V]** The title ships **one** executable, `game_scarlett_master.exe`, plus console DLLs. Its import list (68 names)
  has `d3d12_xs.dll`, `xg_xs.dll`, `xcrdapi.dll`, `xapu.dll`, `chad.dll`, `acphal.dll`, `xboxbvhbuilder.dll`,
  `bink2scarlett.dll`, `amd_fidelityfx_loader_xbox.dll`, `libhttpclient.gdk.dll`, `mfreadwrite.dll`. No PC graphics entry
  (`D3D12CreateDevice`, `dxcompiler`, Vulkan) appears; `D3D12XboxCreateDevice` does. There is no PC build to borrow from.
- **[V]** `shaders.rpf` is 393 MB. xcl's design doc **[X]** says its containers carry a DXIL part whose bitcode is
  replaced by a 4-byte `NOBC` marker (reflection and signatures only), next to an `FSSM` blob of RDNA2 machine code.
  Our `victory-shader/src/dxil.rs` rejects a part without LLVM bitcode magic, consistent with that. I did not open the
  archive myself, so the stripped-bitcode point is **[X]**.
- **[V]** `amd_fidelityfx_loader_xbox.dll` exports exactly `ffxCreateContext/Destroy/Configure/Query/Dispatch`, and names
  `amd_fidelityfx_upscaler_xbox.dll` and `..._framegeneration_xbox.dll` (the latter is not in the folder).
- **[V]** `XboxBvhBuilder.dll` is a C++ class (`XboxBvhBuilder(ID3D12Device11*, bool)`) whose strings are the public DXR
  build inputs (`D3D12_RAYTRACING_GEOMETRY_DESC.*`, instance-desc alignment) plus an Xbox "DEHYDRATED" build flag. Its input
  is DXR-shaped; its output is the console node format.
- **[V]** `Bink2Scarlett.dll` exports the public Bink API (`BinkOpen`, `BinkDoFrame`, `BinkCopyToBuffer`, `BinkGPU`...).
  It is the title's own x64 code, so it already runs natively; xcl **[X]** says the GPU-assisted mode is not used.
- **[V]** `victory-game::KNOWN_BUILDS` is empty and `docs/architecture.md` lists no title-code hooks: **we have no
  code-patching infrastructure for the title today.** Allowed by policy: per-build offsets and byte signatures.
- **[V]** `texture.rs` header: detiling applies only to "texture contents the title writes itself" (streamed pages,
  layout-library copies, movie frames, atlases). Render targets and GPU-written textures are already host resources.
- **[V]** `victory-shader` already has its own DXIL bitcode/container emitter (`emit/bitcode.rs`, `container.rs`,
  `validator.rs`). The thing xcl's direct-DXIL spec delivers (no HLSL, no DXC at runtime) is already ours.

## The seven questions

### (1) Hooking the title's renderer above the console command list
- **What it removes [M]:** list recording is 36,701 calls / 231 ms, but spread over the title's workers and **0.0 ms on the
  render thread**. The wall is memory mirroring (submit thread 74 ms). So even a perfect bypass of per-call translation
  buys little wall time. **[E]** 0-7 ms (THIN's measured effect in cool-fox was 75 -> 68 ms **[X]**, other machine).
- **Feasibility: low.** Our translated shaders expect console root layouts, raw 40-bit addresses and descriptors written by
  the title. To issue "native D3D12 above the list" you must understand the engine's draw-item and descriptor
  abstractions and re-implement them: a renderer rewrite behind byte signatures that break on any title update, on a
  binary we have no symbols for. Effort: months. Risk: high (correctness, version fragility).
- **How others do it:** xcl and cool-fox both wrap D3D12.X faithfully at the vtable and make the wrapper cheaper (xcl:
  UMA, direct DXIL; cool-fox: THIN, descriptors left to draws, parallel hashing). Neither hooks the title's renderer **[X]**
  (I read their docs and our architecture, not every source file).
- **What is feasible instead:** hook at **DLL seams**, which have named, stable exports: the FFX loader (item 6), the BVH
  builder (item 4), Bink, XG. These need no title-code patching.

### (2) PC-native shaders
- **No.** There is no PC shader set (verified: one console exe, console containers; bitcode stripped **[X]**). Source for
  the shaders is not on disk. The only authoritative input is RDNA2 ISA, so translation is the only path.
- What remains is making translation better, not bypassing it: [M] census says all divergence overhead is 16.5 % of
  instructions (ceiling 13.2 fps even if it vanished); `per-lane.md` says 4-8x on "shaders that matter". These two
  statements are unreconciled; the "3x per FLOP" figure is an inference. Do not treat it as measured.

### (3) Native resources instead of console tiled memory
- **Textures GPU-only: already native [V].** Remaining cost is title-written textures: `textures` 6 ms steady **[M]**,
  but hundreds of ms/frame while streaming **[M, texture-load notes: unverified agent write-up]**.
- **Option A, GPU detile:** upload the console-tiled bytes raw and detile in a compute shader (the GFX10 swizzle is a GF(2)
  XOR table: offset = ox[x]^oy[y]^oz[z] in xcl's `tiling.rs` **[X]**, so a table-driven shader is simple). Removes the CPU
  detile and staging copy: **[E]** ~4-6 ms steady CPU, more in streaming. Effort days-1 week. Risk low-medium
  (3 of 912 retail volume LUTs did not match exactly in xcl **[X]**).
- **Option B, decode BCPack straight into a host-side upload, skipping console memory:** only if the DStorage
  destination is known to be GPU-only. The title can also CPU-read textures (ocean displacement, physics sampling,
  `texback.rs`), so you need an allow-list. Effort weeks; risk medium.
- **Option C, the big one (buffers):** the 42.8 ms copy-back and 26 ms of hash/compare lists exist to find GPU writes.
  `design/uma.md` cites a 49-minute sample where **0.19 % of stores were read by anything other than our own refreshes**
  (provenance unverified by me). Making VRAM buffers authoritative for GPU-written data, with an explicit read-back list for
  known readers, is the largest native-versus-emulated win. **[E]** up to ~65 ms submit thread and ~26 ms card.
  Caveat from the same doc: opened system-memory heaps measured 146 ms vs 131 ms (this i7-10700KF is PCIe 3.0), so
  keep data in VRAM, not in opened host heaps. Effort weeks; risk high (a missed reader is a silent wrong answer).

### (4) Native DXR for the BVH builder
- **Not recommended.** The title does not call a ray-tracing API. Its traversal is 64 title compute programs that read
  console BVH nodes by raw address and use `image_bvh*_intersect_ray` (`fork-ray-tracing.md`). The static world's
  structures ship **pre-built in the console node format**; DXR cannot ingest them (its format is opaque).
- The builder's DXR-shaped *input* is a tempting seam, but you would also have to replace all trace programs with
  `RayQuery` or `DispatchRays`: re-authoring shaders whose source you lack, plus re-deriving geometry for static
  structures. Effort months; risk very high.
- Current cost **[M]**: builder on ~+40 ms card time in the dorm (13.3 ms `CopyForBVHUpdate`, 24 ms traces); off by default,
  `VICTORY_AMBIENT` stands in. Cheaper options: keep it off, tune the existing stand-in. A 4070 Ti's RT cores would win big
  *if* the above were done; not worth it now.

### (5) Bink, audio, DirectStorage
- **Bink:** nothing to replace. It is the title's own native x64 code decoding on the CPU; only the output texture upload
  costs, and it is not in the gameplay frame. A PC Bink is proprietary and not owned. **Gain ~0 ms.**
- **Audio:** XMA decoded on our own thread, Opus via `opus_pure`, mixed to WASAPI (`audio.rs`, `apu.rs`). Off the
  critical path; no measured cost. One functional gap: `chad.dll` reverb returns silence. A host convolution would fix
  a feature, not overhead.
- **DirectStorage GPU decompression:** PC DirectStorage GPU-decompresses only GDeflate. The data is zlib + BCPack, so it
  does not apply as shipped. A transcode to GDeflate + plain BC in a derived cache is possible but ties up disk (72 GB
  `levels.rpf`) and is a loading-time win only. **[E]** 0 ms steady state; effort weeks; low priority.

### (6) Upscaling (mention only)
The cleanest seam anywhere: replace the loader's five exports with HLE functions so the console upscaler DLL (which
calls device method 0x218 and measures 0.115 ms/draw vs 0.029) is never entered. `design/dlss.md` already designs this
(option A). Needs a PC upscaler binary the user does not hold (see legal note).

### (7) xcl features that bypass a layer we still have
- Direct DXIL (spec `2026-11-24-direct-dxil.md`, in git history, not the working tree): **already ours** (`emit/`). xcl's
  sweep **[X]**: 197 of 200 sampled programs translate, DXIL 1.27x larger than HLSL+DXC, p50 64 ms.
- UMA windows (`gpu/uma.rs`): not ours yet (our `inplace.rs` places only contiguous runs; 0 regions placed because the title's
  heap is mapped page by page, blocker 3). xcl's window scheme (up to 4 windows at 4 GiB strides, shader address
  translation) targets the same copies; caveat above on PCIe 3.0.
- GFX10 tiling tables, BCPack, DStorage: we have equivalents (`victory-formats`, `dstorage.rs`).
- Nothing else in xcl bypasses an emulation layer; its docs record Bink GPU mode and BCPack hardware swizzle as unsupported.

## Ranking (ms removed x confidence / effort)

| # | Item | Removes | Effort | Risk | Basis |
|---|---|---|---|---|---|
| 1 | VRAM-authoritative buffers, read-back allow-list (3C, UMA variant) | up to ~65 ms thread + ~26 ms card | weeks | high | [M] costs, [E] gain |
| 2 | Intercept FFX loader, PC upscaler (6) | [E] 2-5 ms card, unblocks 0x218 | days | low-med; needs binary user lacks | [E] |
| 3 | GPU detile compute (3A) | [E] 4-6 ms steady, more streaming | days-week | low-med | [M] 6 ms |
| 4 | Finish THIN (not a bypass, from cool-fox) | [E] 0-7 ms | days-weeks | med | [X] |
| 5 | BCPack-direct-to-host (3B) | streaming stalls | weeks | med | [E] |
| 6 | GDeflate transcode (5) | load time only | weeks | low | [E] |
| 7 | Hook title renderer (1) | 0-7 ms wall | months | high | [E] |
| 8 | DXR (4), PC shaders (2), PC Bink (5) | not achievable / ~0 | n/a | n/a | [V] |
