# Caching options: load time, stutter, steady-state frame time

Read-only research. Tree: D:\victory-work\wt-bisect (code paths below also checked in wtv-main).
V = verified in code or on disk. E = estimate.

## Facts verified on disk (V)
- cache-fx/pipelines: host-pipelines.bin 1,052,892,498 B (1.05 GB), early-pipelines.txt 270 KB (docs say 68 KB: list has grown), slow-pipelines.txt 30 KB.
- cache-fx/shaders: 256 sharded subdirs; shard 00 holds 897 files / 42.5 MB. Extrapolated about 230k entries / about 10.9 GB (E, one shard sampled). Files dated Nov 25 and Nov 26: entries from several translator keys coexist (stale keys are never purged). The census says only 1,433 programs matter in the world.
- cache-fx/images: 6 DLLs (title image hooks, one 120 MB). This is not a texture cache.
- NVIDIA DXCache (%USERPROFILE%\AppData\Local\NVIDIA\DXCache): **32 GB**. The driver's default cap is far lower, so something raised it (or it is unbounded). No code in the tree touches the driver cache (no DISABLE_DRIVER_CACHE, no CachedPSO).

## Per option

### 1. Shader translation cache + `--warm` + precompile pass
- Exists (V): `crates/victory-gpu/src/shader_cache.rs` (content-addressed, key = container + translation fingerprint + this file's source; atomic writes; HITS/MISSES counters). `victory translate --warm <hashfile|all>` in `apps/victory/src/translate.rs:159` walks the game archives, 4 threads default (`--jobs`), `VICTORY_WARM_SECONDS` limit, resumable.
- Missing: nothing automatic. Warm is manual, and the key changes whenever the translator crate or shader_cache.rs changes, so any translator edit invalidates all 230k entries and gives the "2,400 misses fresh" stall. No purge of stale keys. No first-launch hook. Default 4 jobs is low for an i7-10700KF (16 threads).
- Effect (E): removes the 2,400-miss cost entirely, about 333 to 157 ms/frame during load per the sibling measurement. Steady-state: none.
- Effort: low. Run `--warm all --jobs 12` after each translator change, plus a "stale key sweep". A first-launch auto-warm is a small addition in `run`.
- Risk: low. Disk grows (see above). Do not run during a measurement.

### 2. D3D12 pipeline library + driver cache
- Exists (V): `pipeline/library.rs` (865 lines): library keyed by a hash of host description, mmap'd, saved atomically every 20 s; `early-pipelines.txt` handed to workers at creation (cap `EARLY_MAX=8192`); per-name locks (not global); trim of mapped pages.
- Measured in docs/pipeline-stalls.md (V): `made=0 loaded=2495 started_early=2504`: the world stall is **library loads, not compiles** (2-3 s frames, `passes 2218 ms`, 3,000+ draws dropped).
- Missing: (a) the world's pipelines are not in the early list soon enough (docs' lever 1); (b) load is paced by driver deserialisation; the 1.05 GB library is mapped and read for 2,500 pipelines, so it is partly bound by the SATA SSD on a cold file cache (E); (c) library is invalidated by any driver update; (d) no driver-cache policy: 32 GB DXCache is suspicious (E: driver lookup overhead, never inspected).
- Effect (E): starting the world's set in the first seconds of load (during the loading screen) should convert multi-second stalls and about 3,000 dropped draws into zero. Largest stutter item.
- Effort: low-medium (reorder/extend early list; check the list includes the world scene; pre-read library file to warm OS cache, see option 7).
- Risk: low; the library file has had zero-page/device-removed history (documented in library.rs), so keep the handle-open rule.

### 3. Decoded BCPack/zlib texture cache
- Exists (V): nothing. `dstorage.rs` reads (`seek_read` on a normal handle, so the OS cache is in use), inflates, BCPack-decodes on a worker pool, writes into title memory. `tiles.rs` and `texture.rs` then detile and upload.
- Missing: any store of decoded output.
- Effect (E): removes CPU decode only. 10.6 GB written in a session at about 550 MB/s from SATA = 19 s of pure reads at best; decoded BCn is larger than BCPack (about 1.5-2x, E), so a decoded cache **reads more bytes from the slow disk than the compressed source**. It only wins if decode is the bottleneck, which is unproven: dstorage logs `busy_workers/reading_workers/bcpack_workers` ratios (V, line ~552), so read them from an existing log before building anything.
- Effort: medium-high (key by file+offset+len, handle 8,128 unmapped skips, invalidation). Risk: medium. Disk cost: tens of GB. Recommendation: do not build until the decode ratio says CPU-bound.

### 4. GPU-resident texture cache + eviction
- Exists (V): host copies persist per texture across frames (a texture is re-detiled/uploaded only on change; `texture.rs` header). `tiles.rs` (reserved resources, levels on demand, off unless `VICTORY_LEVELS_ON_DEMAND=1`) bounds VRAM by holding only mapped levels. `resource.rs` ~1850 already computes "idle for 30 s" and "unmapped" copies as a **diagnostic only**; no MakeResident/Evict anywhere (grep, V). tiles.rs states "tiles are not taken back while the host copy lives".
- Missing: eviction of idle host copies; a texture change re-uploads **every level** (shortcut documented), which plausibly explains much of 11.4 GB uploaded for 10.6 GB written (E).
- Effect (E): partial-level/page uploads could cut the upload volume and the 6 ms texture prep; eviction helps only if VRAM pressure appears (12 GB card, 11 targets, CARD_WINDOWS_MB=2048 and CARD_BUFFERS_MB=1280 already reserved).
- Effort: medium (upload changed levels only) / high (eviction). Risk: medium (black-texture failure mode is documented in tiles.rs).

### 5. Skip unchanged ranges (hashes/host copies)
- Exists (V): write-watch dirty tracking at 64 KiB (`watch.rs`), `copy_changed` compares and stores only differing chunks (`shadow.rs` ~3740), `VICTORY_LIGHT_REFRESH` answers from a journal with no lock or syscall, `CARD_REHASH_RUNS`, `CARD_HASH_LEN`.
- Missing: LIGHT_REFRESH is unfinished and crashed in the all-on set (CLAUDE.md); the 14 GB/frame refresh hash (14,405 refreshes, 0 skipped) is the target. Per CLAUDE.md a copy costs about 5.5 us of overhead and about 1 us of bytes, so skipping bytes does not help; only skipping operations does.
- Effect (E): this is the only option that attacks the roughly 65 ms of the 127 ms frame. If LIGHT_REFRESH removed even a third of the operations, 15-25 ms (E).
- Effort: medium (finish and isolate-test). Risk: high (startup crash seen; correctness of stale data). Measure alone, one switch, per repo rules.

### 6. Cache translated command lists / descriptor tables / root signatures
- Exists (V): descriptor host heaps keep slots made from records and only remake changed ones (`descriptor/host.rs` `made`/`copied` fingerprints); root signature serialised by host `D3D12SerializeRootSignature` (`root.rs:95`) with no visible dedupe cache (not confirmed absent in other files); pipelines use library.
- Missing: list-call recording is 243 ms over 20k calls but runs on the title's workers, 0.0 ms on the render thread (CLAUDE.md), so caching lists does not shorten the frame. Replay of lists is not possible because contents change per frame.
- Effect (E): negligible for frame; a root-signature hash-dedupe is a small CPU saving. Effort low for roots, high for lists. Skip; THIN port is the real fix and is separate.

### 7. OS file cache / prefetch archives into RAM
- Exists (V): nothing: no prefetch, no cache hints (`CreateFileW` flags 0). Windows file cache already serves repeat reads automatically.
- Effect (E): 64 GB RAM cannot hold 209 GB, but the world-load working set (10.6 GB of title data plus the 1.05 GB pipeline library plus shaders.rpf 411 MB) fits easily. Pre-reading those on a background thread at startup (or `cat > nul`) turns 550 MB/s into RAM speed on a second run, and warms the library mapping that option 2 stalls on. Probably the best load-time win per effort for a cold boot; useless if the cache is already warm (it is after a prior run unless RAM pressure).
- Effort: low (script: read the files the walls trace lists, `VICTORY_WALLS_TRACE=2`, dstorage.rs ~448). Risk: very low. Needs a trace to know the exact list.

## Ranking by value per effort
1. Warm shader cache every time the translator changes (`--warm all --jobs 12`): kills the fresh-cache stall. Low effort.
2. Fix world pipelines in the early list + pre-read the library file (options 2 and 7). Low effort, removes multi-second stalls and about 3,000 dropped draws.
3. Prefetch the load working set into RAM (option 7). Low effort, helps cold load.
4. Check the 32 GB NVIDIA DXCache and the dstorage busy/reading ratios in an existing log (no code), to decide on option 3.
5. Finish LIGHT_REFRESH (option 5): biggest steady-state lever, medium effort, high risk.
6. Level-granular texture upload (option 4a). Medium.
7. Decoded texture cache (option 3), eviction (4b), command-list caching (6): low value now.

Unverified: DXCache cause, real shader-cache total size, which pipelines the world adds late, whether decode or disk bounds texture streaming.
