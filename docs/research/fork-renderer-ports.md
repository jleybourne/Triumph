# The RX 7900 XTX fork renderer ports 1-7 vs our fast-integ tree

Sources: `git log/show the RX 7900 XTX fork`. reports/PORT-PATCH-*.md are not on that branch, so figures come from README and commit bodies. Code exists for ports 1, 2, 5, 6, 7 only (no 3; "patch 4" is folded into 7). Gains "here" are my estimates.

**Port 5 (CBV raw).** Translated shaders read constant buffers as raw bytes (raw twin per slot, root SRV), so the title's CBs are read in place: ~22,000 copies/frame to 0. Theirs: -4.7 ms. Ours: nothing (no constants_raw in victory-shader). Works on NVIDIA (the "no end on AMD" caveat is AMD-only). Effort medium-high (emitter, descriptor host, shader cache key; needs in-place memory, which INPLACE=2 gives). Est. here: 5-12 ms.

**Port 6 (guards).** GUARDS becomes an RwLock; faults and refreshes share it with per-64 KiB BUSY claims. View memory is not asked of the kernel. Refresh planning (first-write spans, which refreshes copy nothing; 2.9+3.9 ms, up to 6 ms in one submission) is folded per list on helper threads and joined in order. Theirs: 47.2 to 45.1 ms, title-thread waits 29 to ~2 ms. Ours: `GUARDS` is a single Mutex (watch.rs:705); LIGHT_REFRESH skips the lock when journal-answerable; planning is serial. Pure CPU, applies. Effort medium: lock redesign in watch.rs (their watch.rs diverges ~4.8k lines from ours), but the plan fold/join is self-contained. Est. here: 3-10 ms, likely more if our +100 ms zero-copy bookkeeping is lock-bound (check lane wait profile first).

**Port 2 (descriptors).** Binary-patches the title's six descriptor-record stores (plus memcpy/memmove) to mark slots; sync_heaps reads marks instead of ~290 watch pages/~600k slots. Theirs: removed 15-16 ms/frame of heap-sync thread (5-7 ms watch queries); lane wait ~6 ms. Ours: page-scan sync_heaps (descriptor.rs:183). Applies; effort medium (loader patch sites byte-checked, mark maps, fallback audit). Est. here: 2-6 ms of lane wait, plus freed cores.

**Port 7 (journal).** Heap records requested at the title's submit; journal split into 64 lock-sharded parts. Theirs: -0.7 ms (lane waits ~6 to 0.3). Ours: single JOURNAL Mutex (watch.rs:81). Applies; effort low-medium. Est. here: 1-3 ms; do after 6.

**Port 1 (title heap in place).** Already ported; places 0 regions here (page-by-page allocator). Value is groundwork for port 5; gain now 0.

## Ranking (gain per effort)
1. Port 6, 2. Port 5 (largest proven single win, more work), 3. Port 2, 4. Port 7, 5. Port 1 (only to make port 5 bite).

## What got them 280 to 45 that we lack
Their findings table: descriptor records left to draws 280 to 124 (biggest); preparing on its own thread 103 to 75; THIN list calls 75 to 68; stores into guarded memory looked at first, fewer guards reset 69 ms thread time to ~6. We have ASYNC_SUBMIT/JOIN_PARTS/spread-like pieces but no descriptor marks, no sharded guard/journal locks, no parallel planning, no in-place CBs. Their principle: avoid noticing/moving (marks, in-place), not optimize bookkeeping. Our zero-copy step went the other way (+100 ms bookkeeping).

Caveats: GPL-3.0, credit the fork in commits. Port 5 assumes the title writes constants before submit (ring + fences). Unverified: whether our draw path still copies CBs (grep found no `defer_constants`).
