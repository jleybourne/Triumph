# GPU texture path: detile on the card, DirectStorage direct, level upload, red-block triage

Read-only design. Tree: D:\victory-work\wt-bisect. V = read in code, E = estimate (not measured). Nothing here was run.

## Current path (V)
- DirectStorage: `crates/victory-hle/src/modules/dstorage.rs` `execute` (read, zlib, BCPack) -> `into_destination` -> `victory_gpu::kernel_will_write` (opens guarded blocks), write in place (or `xmem::write_physical` / `pages_written` for page destinations, up to 12 retries), then `title_memory_written`. BCPack output is the console-TILED block stream.
- Refresh: `crates/victory-gpu/src/texture.rs` `Staging::refresh_timed`: `watch::writes` (or a guard plus a hash every 8th frame for view memory) -> `copy_mapped` / `blake3::hash` of the whole texture (unmapped: re-read at most every 30 frames) -> `Staging::detile` loops every subresource -> `Layout::detile` (`victory-formats/src/texture.rs`, `walk`) into a mapped staging buffer -> `upload()` records one `CopyTextureRegion` per subresource (every mip, every slice) -> host texture. `dirty` is one bool per texture.
- Tiling modes (`Swizzle` in victory-formats): LINEAR; 256B_S/D (1,2); 4KB_S/D (5,6); 64KB_S/D (9,10). `addressable()` is false for the XOR modes (17/18, 21-27: `_T`, `_X`, Z_X), left to render targets and depth. So the CPU detile covers only the non-XOR modes; `texture not detiled` and `texture contents without a console layout not uploaded` warnings show what is left. The address is a GF(2) table: `column(x)` + `row(y,z)` = `(block index << log2 block bytes) | pattern[axis][i]`, tail mips shifted by `Tail{x,y}`.
- Mips of a slice are stored smallest first, mip 0 last, so each mip is a contiguous range (`Mip.offset`, `slice_size`); tail mips share one block.

## 1. GPU detile
Upload the tiled bytes as they are; a compute shader writes the linear footprint layout.
- Files: new `crates/victory-gpu/src/detile.rs` (pipeline, per-layout table buffer, dispatch) plus a compute kernel. Changes in `texture.rs`: `Staging::detile`, `upload()`, `staging()`. `Layout` gains an export of `pattern[0..3]`, `block`, alignment log2, per-mip `{offset, pitch, rows, tail x/y}`.
- Kernel: one thread per element run (16 bytes for BC), address exactly as `walk`. Input: raw bytes in an upload-heap `ByteAddressBuffer`. Output: a default-heap buffer in the placed-footprint layout `footprints` already describes, then the existing `CopyTextureRegion` (BC formats cannot be UAVs, so do not write the texture directly). Tables in a shared buffer keyed by layout hash.
- Keeps the write-watch and the hash. Removes the CPU walk. Raw upload is a little larger than linear (block padding, tails): E +5-15 percent PCIe bytes.
- Effect: E 2-4 ms of the 4-6 ms refresh (the rest is hash and `copy_mapped`, which only item 2 removes); card cost E 0.1-0.4 ms a frame. Backlog: detile is the refresh throughput limit; E 2-3x faster drain while streaming, but nothing for the 8,128 unmapped skips (a mapping state, not a cost).
- Effort: about 1 week (kernel, tables, golden test against `Layout::detile` for each addressable mode and format; on the card like `tiles.rs` with `VICTORY_TILES_TEST_CARD=1`). XOR modes later only if a sampled texture needs them.
- Risk: low-medium. 3 of 912 retail 3D layouts did not match in a sibling project (research-native-paths.md): self-check each layout against `Layout::detile` at first use and fall back to the CPU on mismatch.
- Switch: `VICTORY_GPU_DETILE=1` (default off); `=verify` does both and compares (diagnostics build).

## 2. DirectStorage straight to the card
BCPack output is tiled, so "direct" still needs a detile: first on the worker thread, then on the GPU (item 1).
- Hook: in `dstorage.rs` `execute`, before `into_destination`, ask `victory_gpu::texture::direct_target(dest_addr, len)`. It returns the texture, its `Layout` and the mips the range covers exactly (`Mip.offset`, `slice_size`); partial coverage returns None and the old path runs. Needs an address-range index of placed textures built at creation (`resource::texture_memory`), not at first sample (`staging()` is lazy). Textures created after their data arrives fall back: that is the 8,128 case, so expect partial coverage.
- Path A (stage 3a): the worker decodes into a pooled staging buffer, runs `Layout::detile` there (off the submit thread, parallel on 16 threads), marks the mips ready; `upload()` copies those subresources only.
- Path B (stage 3b): the worker decodes into an upload-heap buffer in tiled form; the GPU detiles (item 1).
- Title memory correctness: the title (and `texback.rs`: ocean, physics) may read the bytes. Skip the memory write only when ALL hold: block-compressed format; not a kind in `VICTORY_TEXTURE_BACK`; no read seen in a probation run (`VICTORY_READ_WATCH=1` exists) and on an allow-list built from it. Everything else writes memory as today. Safety net: keep the decoded bytes (they are the upload source anyway), read-guard the range (`PAGE_NOACCESS`) and materialise on the first read fault. First check that `watch.rs` guards support read faults; if not, start with `VICTORY_DS_DIRECT=probe` (both writes, compare only).
- Removes for covered textures: `kernel_will_write`, `title_memory_written`, guard reopening faults, `copy_mapped`, the whole-texture hash, and the title-memory write (E up to 10.6 GB a session).
- Effect: E 3-5 ms of refresh on the submit thread (worker load rises instead). Backlog: E drains most of the 12,030 waiting once covered.
- Effort: 2-3 weeks (range index, mip coverage, probation list, read-fault path, ordering against the sampling frame).
- Risk: medium-high. A missed CPU reader is a silent wrong answer (boats sank in the ocean case). Also an upload landing before the title remaps pages, and a destination reused by a different texture (stale host copy).
- Switch: `VICTORY_DS_DIRECT=0|probe|1` (default 0), `VICTORY_DS_DIRECT_FORMATS=bc1,bc3,...`.

## 3. Level-granular upload
- Hint from the numbers: 11.4 GB uploaded against 10.6 GB written is about 1.08x, so whole-texture re-uploads are not the main volume. The saving is on textures that stream mip by mip or are partly rewritten: E 10-30 percent of upload bytes, E 0.3-1 ms a frame.
- Files: `texture.rs` `Staging.dirty` (AtomicBool) -> `dirty_mips: AtomicU64` (bit per mip, all slices). `refresh_timed` maps `watch::writes` ranges to mips via `Layout.mips[i].offset` and `slice_size`; `Staging::detile` and `upload()` loop only over set bits; the `defer_host` closure clears only the bits it recorded (`fetch_and`) so a write between recording and submit survives. Hash paths: hash per mip range, which is also cheaper. Levels on demand (`tiles.rs`, `detiled` mask) already works per level: join the masks. Tail mips share a block: mark them together.
- Effort: 2-3 days. Risk: low (a wrong range-to-mip mapping leaves a stale mip: add a verify mode that uploads all and compares).
- Switch: `VICTORY_TEXTURE_LEVELS=1`.

## 4. Red block artefacts: texture or shader
Blocks of 4x4 or fixed 8/16/32-byte chunks point at the texture side; whole-surface or lighting-shaped errors point at the shader.
1. Capture the bad frame: `VICTORY_CAPTURE_TOOL=renderdoc`, `VICTORY_CAPTURE_AT=<frame>` (or F11). Files land in `D:\victory-work\captures\rdc\`.
2. In RenderDoc pick a clothing draw, run Pixel History on a red pixel, and inspect the bound SRVs in the Texture Viewer, mip by mip, at the mip the draw samples. Artefact inside the SRV, aligned to 4x4 blocks: texture. SRV clean but output wrong: shader. Clean in a later frame: timing (30-frame partial re-read, late mip upload).
3. Shader side: `VICTORY_SKIP_PIPELINES=<16-digit shader hash>` (hashes from the pipeline diagnostics; diagnostics build only). A vanished draw proves little alone: skip post-process pipelines one at a time to see whether the red is made in the base pass or added later, and read it together with step 2.
4. Texture-side switches, one at a time at the same spot: `VICTORY_STAGING_POOL=0` (pooled staging buffer reused with stale content), `VICTORY_TEXTURE_GUARD=0`, `VICTORY_TEXTURE_COPY=1`, `VICTORY_LEVELS_ON_DEMAND` off (a reused tile showing its last owner's data looks exactly like blocks), `VICTORY_TEXTURE_NO_WAIT` off. Also dump the tiled bytes at the draw and detile them with `Layout::detile`.
5. Candidates: BCPack decode of BC3/BC5/BC7 (`victory-formats/src/bcpack.rs`; BC2 is not decoded at all; a wrong BC7 mode or BC5 channel gives red/green blocks); a failed decode leaving zeros or old data (`a sampled texture's memory is all zero` log); a partly mapped texture uploaded with some mips; an XOR mode not detiled.
- Effort: 1 day. Risk: none.

## Staged plan
0. (1 day) Item 4 triage, plus counters for DirectStorage destination classes (covered, partial, unmapped) via `VICTORY_DS_DIRECT=probe`.
1. (3 days) Level-granular upload, `VICTORY_TEXTURE_LEVELS`.
2. (1 week) GPU detile behind `VICTORY_GPU_DETILE`, golden tests per mode.
3. (2-3 weeks) DS direct: path A on workers with the allow-list, then path B with the GPU detile.
4. Remeasure after each stage: `RefreshUs`, upload bytes, waiting and unmapped counts. If the red is a decode bug, fix it before stage 3, since stage 3 changes who consumes the decoded bytes.
