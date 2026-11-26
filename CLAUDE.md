# CLAUDE.md — the loader and translator for "new game 6"

Read this before touching anything. It is the state of the work as measured, not as intended. Every
number below came out of a run on the machine described in *The machine*, and the run it came from is
named in `docs/runs.md`.

**The title is never named in this repository.** It is "the title" or "new game 6". No strings matching
`grand theft`, `rockstar` or `take-two` may be added to code, docs, commit messages or issues.

## What this is

A console title's graphics and system layer, reimplemented for Windows: the executing title is a
PowerPC/AMD-console binary, and this repository provides the host side of its API — the GPU translator
(its command lists, descriptors, shaders and memory model), the system calls it makes, and the memory
model that stands in for the console's unified memory.

```
crates/victory-gpu      the GPU side: device, lists, draws, descriptors, copies, shadows, watch
crates/victory-hle      the system side: kernel, files, xmem, display, lifecycle, dstorage
crates/victory-shader   shader translation: console ISA -> DXIL
crates/victory-image    the title's own image decoder hooks
apps/victory            the CLI: translate (offline), run, census, shader-info
docs/                   measured findings; read these before proposing work
```

## The machine

```
CPU     i7-10700KF          GPU  RTX 4070 Ti, 12,282 MiB
RAM     64 GB               BAR1 16,384 MiB, 16,355 used (ReBAR healthy, measured)
OS      Windows 11 26300    screen 2194x1234
game    <game data folder> (209 GB; shaders.rpf 411 MB)
work    D:\victory-work\    (logs, captures, worktrees, scripts — never C:\...\Temp)
```

The console's wave size is 64; this card's is 32. Every wave64 operation is split into two 32-lane
halves. That is structural and it is why the same build is slower here than on an RDNA3 card.

## Where a frame goes (measured, diagnostics build, world)

```
frame                 131.3 ms median · 7.6 fps · ~4,500 draws
  preparing           85-110 ms, of which copies 31-48 ms (83-115 MB) and textures 6 ms
  copying back        48-54 ms (60-65 MB)
  on the GPU          ~90 ms busy (diffuse: no pass or draw kind above a few ms)
  list calls          20,293 per frame = 243.2 ms of work on the submitting thread
```

Best frame recorded: **8.9 fps / 112.2 ms** at `fsrQuality=3`. Session start was 3.4 fps / 295 ms.

**A frame-time win that draws fewer draws is not a win.** Every figure must be reported as
`fps · ms · draws · dropped`.

## What is confirmed to help (and is in the launcher)

The launcher `D:\victory-work\scripts\run_play.ps1` is the confirmed set and nothing else:

| change | measured |
|---|---|
| title memory taken in place (`VICTORY_INPLACE=2`, `INPLACE_MORE`, `INPLACE_VIEW`) | copies 75.4 → 40.0 ms, copy-back 73.9 → 46.6 |
| page pool opened as heaps (`VICTORY_XMEM_COMMIT=1`) | the `SEC_RESERVE` blocker removed |
| the submission/card set (`ASYNC_SUBMIT`, `QUEUE2`, `INDIRECT_WINDOW`, `TEXTURE_NO_WAIT`, `MERGE_GAP=64`, `MERGE_RUNS=4096`, `STAGING_POOL=0`, `COPY_PLAIN`, `JOIN_PARTS`, `BACK_OWN_VIEW`, `INDIRECT_GATHER`, `CARD_REHASH_RUNS`, `CARD_WINDOWS_MB=2048`, `CARD_BUFFERS_MIN_KB=64`, `CARD_BUFFERS_MB=1280`, `TEXTURE_BACK`, `CARD_HASH_LEN`, `FILLS_PLAIN`, `STEP_INDEX`) | the baseline above |
| `fsrQuality=3` in the title's own settings | best frame 117.5 → 112.2 ms; `docs/settings.md` |

## What is measured and does NOT help (do not re-propose these)

- **Copy coalescing.** Four separate attempts; `VICTORY_COPY_MERGE`/`MERGE_RUNS` leave the count flat
  (11,336-11,795), `COPY_PLAIN=0` costs 152.7 ms, `REFRESH_COARSE` −5 % count but +70 % bytes,
  `INPLACE=2 + INPLACE_MORE` 6,460 copies but +63 % bytes. A copy costs **5.5 µs of which ~1 µs is
  bytes** — the cost is the operations, not the traffic. `docs/copies.md`.
- **Double buffering.** There is nothing to alternate: copies write from title memory into the list's
  per-range upload memory, each range feeding the same frame's draws.
- **Barrier dedupe.** Implemented (`VICTORY_DEDUPE_BARRIERS`), never measured, ceiling ~18 ms. The
  other fork's findings: "leaving out barriers: nothing to 0.1 ms each".
- **`VICTORY_PRECOMPILED=1`** (method 0x218): works, but **0.115 ms/draw against 0.029** from the
  library. Off by default, with the reason in the code.
- **`safecopy`** (`rep movsb` instead of kernel copies): ported and crashed, then fixed, then measured —
  no gain, because copies are already made in place (`7,606 in place, 0.6 met a page that was not
  there`), and the run degraded 141 → 467 ms at the same draw count. Reverted. The crash it caused is
  a lesson kept below: **our fault handler only takes write faults**, so anything that faults on a
  *read* (as `rep movsb` does when the title unmaps a page under it) reaches the title and kills it.
- **`vrsEnabled=true`**: the title makes **zero** shading-rate calls, so it is inert (`docs/settings.md`).
- **`fsrQuality=4`**: black screen. The one value not to leave in the file.
- **Series S profile**, smaller textures, a second drawing queue, `ASYNC_SUBMIT` off, mapping pages on
  16 threads, scalar buffer loads, wave sizes, fused multiply-adds — each measured by us or by the other
  fork as neutral. Series S additionally renders black here (0 refusals, draws succeed, present texture
  never written).
- **Wave64 translation as a route to 30 fps.** Census: 1,433 programs, 4,641,843 instructions, all
  divergence overhead = 766,759 instructions = **16.5 %**. Even all of it gone: GPU 90.8 → 75.8 ms =
  **13.2 fps**. 60 % of *all* instructions gone: 36 ms = 27 fps. `docs/per-lane.md`.

## The blockers, with their evidence

0. **A startup assert in `4b3fe7e2`.** That commit (the precompiled method-0x218 work) asserts inside
   D3D12 at startup on this machine: `unhandled exception 0x80000003 at D3D12Core.dll+0x11632`, right
   after `host D3D12 device ready`, three runs in a row and no frames. `20025213` — the commit before
   it — runs: `4,270 draws`, no crash. **Until that is diagnosed, 20025213 is the playable build** and
   the precompiled work stays on branch `fsr` for whoever fixes it next. Do not read the assert as a
   setting: it happens before any draw, with no `VICTORY_D3D12_DEBUG` set.
1. **List calls: 20,293 a frame, 243.2 ms on one thread.** The other fork's `VICTORY_THIN` writes the
   calls down and translates them on 8 threads: 36,000 calls = 230 ms over 8 threads, 75 → 68 ms of
   frame. Porting it needs `crates::cores::keep_many_on_fast_cores`, `queue::line_on`,
   `queue::line_moved`, and a `boxed()` list constructor that this tree does not have — ten compile
   errors when attempted; `crates/victory-gpu/src/list/thin.rs` is present but not declared.
2. **The upscaler.** The title's FSR passes call device method 0x218; with `VICTORY_PRECOMPILED=1` they
   are made from the containers (log: `calls=1,2,3,4,8`), but the path is 4× more expensive per draw.
   `fsrQuality` 4 never presents a picture. The internal render scale is otherwise unreachable: the
   title derives it from its own settings file, not from the display mode we report (426×240 reported,
   GPU unchanged).
3. **Zero-copy of the big card buffers.** Placement works (`26,393 views of placed memory`), but the
   port places **0 regions** because `inplace::stretch()` asks for one contiguous run of one section
   and the title's 32-bit heap is mapped page by page (`Cover::Scattered`). The other fork maps one
   185 MB view of the whole heap.
4. **348-549 draws a frame dropped** on four compute pipelines (~87 draws each, 8 % of the scene):
   `E_INVALIDARG (0x80070057)` at PSO creation. Cause found: static samplers combined with
   `SAMPLER_HEAP_DIRECTLY_INDEXED` is forbidden; the fix (drop only the sampler flag when static
   samplers exist) is implemented in `wt-rs` and has never been measured in world.
5. **The 14 GB refresh path.** `14405 refreshes 14112.5 MB, 0 skipped` per frame against ~83 MB copied:
   the refresh ranges are hashed, and that lands inside `preparing`. `VICTORY_LIGHT_REFRESH` exists and
   checking it is unfinished.

## How to build and run

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --profile diag --features diagnostics -p victory --target-dir target/vd3
cp target/vd3/diag/victory.exe /d/Developer/fxceless-victory/target/diag/victory.exe
```

**Only diagnostics builds are measurable**; the release build reaches 13 draws and cannot be driven by
the harness (`docs/runs.md` row 22).

```powershell
D:\victory-work\scripts\run_play.ps1        # the confirmed set only, 900 s
D:\victory-work\scripts\run_viewplace.ps1   # the same set with INPLACE_VIEW, used for A/B runs
D:\victory-work\scripts\run-fx.ps1 -Diag -Seconds 600 [-WindowScale ..] [-Roam ..]
```

**Stage the playable build, not the newest one.** `wt-verify-port2` at `20025213` builds to
`target/vd3/diag/victory.exe` and runs; the commit after it asserts at startup (blocker 0). Copy the
exe to `D:\Developer\fxceless-victory\target\diag\victory.exe` before any launcher.

**Do not put an unmeasured switch in the launcher "because it might help".** The all-on set
(`LIGHT_REFRESH` + `EARLY_WORLD` + `PORT_CONSTANTS` + `HEAP_SYNC_TABLES` + `LATE_WAITS` on top of the
confirmed set) crashed the title at startup, and `PRECOMPILED` and `safecopy` each measured worse than
not having them. A switch goes in the launcher only after a run that beat the baseline with the same
draw count.

**One instance only.** A live `victory` process makes the next launch exit immediately and the parser
then reads the **stale** log — this has caused three false conclusions. Confirm the process is gone and
the log timestamp is new before trusting a number.

World load takes 2-4 minutes; a 300 s run never reaches steady state. Use 600 s minimum.

## Rules that have cost time when broken

- **Verify every claim on disk** (`git show`, `grep`, `ls -l`). Agent results have been fabricated
  repeatedly: a 218-test "pass" for a switch that did not exist, a discarded `merge()`, empty branches.
  A subagent's summary is a claim, not a fact.
- **One setting or switch per measurement**, and a screenshot for anything that touches what is
  rendered. A frame line can read 7 fps while the picture is black.
- **Do not kill a run you are measuring**: `Stop-Process` in a follow-up command has twice been read as
  "the setting crashed".
- Reference code is read, not copied: this repository is GPL-3.0 and the forks differ in licence, so
  reimplement into our own structure.
- All work is pushed to `jleybourne/Triumph`; upstreams are read for updates and PRs only.

## Remotes and branches

```
triumph  jleybourne/Triumph                 (all work lands here)
fx       fxceless/victory-fork              (upstream)
triippy  triippywhale/victory-fork          (9e7a6322: precompiled method 0x218)
zslayer  ZSlayerHQ/victory-world-runtime
belo     Belobug/VIctory-Belobug-AMD        (AMD-specific)
avatar   avatarmanz/VIctory-7900XTX         (86dc1097: the in-place heap port)
```

Branches of note: `main` (memory work + docs), `fsr` (`20025213` port + `4b3fe7e2` precompiled),
`pipeline-drops`, `exact-100` (census), plus several unmeasured agent branches.

## The reference the other fork left behind

`carlm125/cool_fox` (identical tree: `DoubleGhosty/cool_fox`) is an RDNA3 fork that reached 46-65 ms
frames on an RX 7900 XTX and documented its cost structure in `docs/findings.md`. Its measured gains,
in order: descriptor records left to the draws (280 → 124 ms), hash passes over card copies on 64
threads (card 79 → 66), preparing on its own thread (103 → 75), `VICTORY_THIN` (75 → 68), submissions
joined (68 → ~63), AVX2 compares of shader-written bytes (14 → 4-7 ms), and guarded stores looked at
first (69 → ~6 ms of thread time). Its own tree measured **3.5-3.9 fps on this card**, so its figures
describe a 24 GB RDNA3 and are a map of *mechanisms*, not a promise of numbers here.

## What 30 fps actually requires

```
now                       131 ms   (7.6 fps)
both thread-side walls     ~70 ms  (12-14 fps)   THIN + async copy-back + guard resets
and the card halved        30 ms  (30 fps)       the upscaler running natively
```

Neither half is reachable by tuning: the first needs three mechanisms that live in the other fork's
code, the second needs the title's own upscaler to work through method 0x218 without costing 4× a draw.
Anyone claiming 30 fps on this architecture by any other route has not done the arithmetic above.
