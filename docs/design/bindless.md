# Bindless, pull-model buffers, indirect runs and per-pass replay

Design, plus one measuring tool built (`VICTORY_PASS_HASH`, section 7; branch `pass-hash`).
Nothing was run for this document. Tree read: `wt-passhash` at fast-integ `2261b969`
(`victory-shader/src/emit/mod.rs` binding scheme, `lower.rs` `heap_index`/`cbuffer_dword`,
`emit/host.rs` root signature, `victory-gpu/src/draw.rs` `tables`/`bind`, `shadow.rs`, `inplace.rs`,
`shader_cache.rs`, `victory-shader/build.rs`). Inputs: `research-gpu-driven.md` items 1, 2, 4;
`recording-cost.md`; `submit-split.md`; `card-budget.md`; `research-native-paths-2.md`.

Labels: **[M]** measured in a committed log or profile (source named), **[C]** read in code,
**[E]** estimate with its reasoning, **[?]** unknown until the named run.

## 0. Summary

- **The research's 30-60 ms does not apply to this tree as written.** It assumed (a) the frame
  is translation-bound and (b) descriptors are created and copied per draw. Neither holds now:
  - the submit thread (memory sync) is the wall, and recorders idle 70 % of the time
    [M, recording-cost §1];
  - translated shaders **already index the descriptor heap directly** (SM 6.6
    `ResourceDescriptorHeap[]`, title descriptor order kept, tables passed as byte offsets in
    root constants) [C, `emit/mod.rs`]. "Variant A" of the research is done.
- What is left per draw is **our bookkeeping around buffers**: shadow lookups under one global
  lock, refresh registrations, fresh tables for CBVs, `readable` transitions. About 82 ms of
  recorder CPU per present over ~6 threads, with only ~15 ms of it in D3D12 calls
  [M, recording-cost §4].
- The design here removes that bookkeeping in four stages:
  1. **B1: CBVs in place.** This is recording-cost port 5, a prerequisite, not re-designed here.
  2. **B2: pull-model buffers.** Shaders read buffers through an address map over our copies,
     using the console's own 32-byte buffer descriptors from title memory. No host buffer
     descriptor and no per-draw registration.
  3. **B3: ExecuteIndirect runs.** Once a draw's state is "pipeline + root constants", runs of
     draws become one indirect call.
  4. **B4: per-pass replay** of host lists, gated on the pass-hash numbers.
- **Estimates [E]:** frame now 0-3 ms (CPU wall is elsewhere). At the 33 ms target, where the
  recorders become the wall, B1+B2 take 5-10 ms off each recorder thread. B3 takes 1-2 more.
  B4 takes 0 to 10+, depending on the hit rate the tool reports. Card cost +0.5 to +3 ms for
  the address-map indirection.
- **Order:** run `VICTORY_PASS_HASH=1` once (no other cost), then B1. B2 only after the
  memory-sync work makes refresh dirty-driven (submit-split stage 3, native-paths item 2),
  because B2's CPU saving is the per-binding registration, and that cannot go while refresh
  is binding-driven.

## 1. What the tree already does [C]

| piece | today | source |
|---|---|---|
| Textures, samplers, buffer views in tables | `ResourceDescriptorHeap[index]`, where index = console descriptor address / 32 (samplers / 16). The host heap mirrors title slots lazily (`HostHeap::mirror`). | `emit/mod.rs`, `lower.rs` `heap_index` |
| Descriptor tables | Byte offset of the first descriptor, passed in root constants (user data `b0`). No `SetDescriptorTable` per draw. | `emit/mod.rs` |
| Buffer views in the heap | Raw views at the console descriptor's base. Typed-format loads are converted in the shader. The host view must be `R32_TYPELESS` raw at that base. | `emit/mod.rs` |
| Vertex fetch | The shader's own arithmetic over a root SRV at the title's address (stride 1). | `emit/mod.rs` |
| Root CBV/SRV | Root descriptors `b<i>`/`t<i>` in `ROOT_CBV_SPACE`, address = the shadow's GPU address | `emit/mod.rs`, draw.rs `bind` |
| Constant buffers in tables | **A fresh table in dynamic slots per draw, plus a `Fill` per CBV** (12,023 tables a frame [M]) | draw.rs `tables` |
| Title buffers on the card | One host copy (`Shadow`) per title buffer. A title GPU address inside a buffer maps to the same offset in its copy. Refresh covers ranges draws are known to read (registered per binding). | shadow.rs |
| One mirror of all title memory | **Not possible with opened heaps.** "Tile mappings into such a heap are refused and REMOVE THE DEVICE." | inplace.rs |

So "bindless" is done on the shader side. The remaining cost is the CPU's per-binding
registration, and that exists because the buffer side is **push**: the CPU finds a shadow for
every address a draw binds, registers a refresh, and binds the shadow's address. **Pull** turns
this around: the shader translates addresses itself, and the CPU keeps every copy current
whether or not a draw names it.

## 2. Per-draw work that B1-B3 remove (recorder side)

From recording-cost §4 [M unless marked]:

| work in `State::bind` | cost | removed by |
|---|---|---|
| `shadow::BUFFERS` global mutex (lookup per vertex, index, root view, buffer view) | ~30-40 ms/present of futex waits across threads | B2: no lookup at bind |
| Refresh registrations (`Shadow::refresh` → `Deferred::Refresh`) | part of `bind`'s 13-18 µs; also submit-side `Batch::add` | B2b: dirty-driven refresh |
| Fresh CBV tables, SipHash of records, `Create*View` per slot, `Fill` per CBV | 12 k tables/frame; −2-4 ms per recorder thread [E] | B1 (port 5 stage A/B) |
| `readable()` transitions for buffers | small per call [E] | B2: buffers stay in one state (raw SRV/UAV over copies, `COMMON`/UAV) |
| `IASetVertexBuffers`, root views, root constants per draw | ≤1 ms per thread [E] | B3: one indirect call per run |
| Texture `readable` + `mirror` | kept | not changed (hardware sampling needs real descriptors) |

## 3. Stage B2: pull-model buffers

### 3.1 Mechanism

**The address map (GPU).** A structured buffer `AddressMap` with one 8-byte entry per 64 KiB
page of title GPU address space that some copy covers: `(copy heap index: u32, page offset in
the copy: u32)`. An entry of 0 means unmapped (reads give 0, writes are dropped, as the
console's out-of-range buffer accesses). Title GPU addresses are 40-bit, so a flat table over
the full range would be 128 MiB. Instead:

- use a **two-level table**: 4 GiB directory → 64 KiB pages, 8 KiB per populated 4 GiB window;
- each copy (`Shadow::buffer`) gets one **raw UAV** (and one raw SRV) in a reserved range of
  our host heap: `copy heap index`.

Entries are written by the CPU when a shadow is made, retired or remapped (`shadow::BUFFERS`
changes), through one upload per submission. The table lives in card memory, indexed by a root
constant (heap index of the map) set once per list.

**Addresses (shader).** Every buffer access the translator emits through a root CBV/SRV, a
table CBV, or a heap buffer view becomes:

```
va    = base + byte offset                 // base: from the console descriptor (below) or root constant pair
e     = AddressMap[dir[va >> 32] + ((va >> 16) & 0xffff)]
data  = ResourceDescriptorHeap[NonUniformResourceIndex(e.copy)].Load(e.offset + (va & 0xffff))
```

A load that crosses a 64 KiB page is split. The translator knows the access width (≤16 bytes),
so it emits a second lookup only when `(va & 0xffff) > 0xfff0`, a rare branch. Wave-uniform
bases (scalar loads, the usual case on the console) give one lookup per wave: the translator
already knows which addresses are scalar (`self.scalar`), so the lookup is done in the scalar
form and `NonUniformResourceIndex` is dropped.

**The console's own descriptors.** A console buffer descriptor is 4 dwords in title memory
(the descriptor heap the title wrote): 48-bit base, stride, number of records, format, swizzle
and the out-of-range mode. Today the CPU reads them (`HostHeap::mirror`) and makes a host raw
view at the base. In B2 the shader loads the 16 bytes itself, from the **copy of the title's
descriptor heap memory** (itself a title buffer, so it is in the address map), and applies:

- **bounds:** `num_records × stride` (or bytes when stride is 0), checked per access, with the
  console's out-of-range result (0 for loads, dropped stores). Today the host view's size gives
  this implicitly. B2 must emit it, or reads past a buffer read the neighbour's bytes.
- **base:** it replaces the host view's `FirstElement`.
- **format:** already decoded from the instruction (typed-format loads are converted in the
  shader today), so nothing new.

Root CBV/SRV addresses arrive as 64-bit root constants (two dwords each) instead of root
descriptors. That costs one more root dword per view, and a root signature with fewer
descriptors.

**What stays push:** textures, samplers, render targets, depth, UAV textures (hardware
formats, tiling, sampling). Their descriptors stay in the mirrored heap as today, with
`readable` transitions.

### 3.2 Coherence: B2a and B2b

The shader can now read any copy at any address, so "which ranges does this submission read"
is no longer known from the bindings. Two sub-stages:

- **B2a `VICTORY_PULL_BUFFERS=1`.** Shaders pull. The CPU still decodes the console
  descriptors it sees at bind (as `tables` does now) and registers refreshes for them, but
  **without** making host views, transitions or root descriptors. Coherence equals today's
  (including today's shortcut: buffers reached only through dynamically indexed heaps keep
  their first-use copy). CPU saving is small: host view creation and root descriptor binds,
  ~1-2 ms per recorder thread [E]. Its purpose is to prove the translator change and measure
  card cost with no memory-model change.
- **B2b `VICTORY_PULL_BUFFERS=2`.** No per-binding registration at all. At submission every
  copy with write-watch-dirty pages is refreshed (dirty-driven, the "update all dirty" sweep of
  submit-split stage 3). This is where the shadow lookups and registrations leave `bind`.
  Precondition: the dirty sweep must cost less than the registrations it replaces. This needs
  the write-watch rework (native-paths 4d) and wide prepare (submit-split stage 3). It also
  fixes today's coherence shortcut, since every dirty copy is refreshed.

### 3.3 Translator changes (`crates/victory-shader`)

| place | change |
|---|---|
| `lower.rs` `cbuffer_dword` and its 3 call sites; root CBV/SRV handle creation (`root_cbv_handle`) | in pull mode, emit the address-map load instead of `cbufferLoadLegacy` / raw root SRV loads. Uniform addresses keep one lookup per wave. |
| `lower.rs` `heap_index` users for **buffer** views (raw/structured/typed buffer loads and stores, buffer atomics) | load the console descriptor's 16 bytes through the map at `heap_base + index × 32`, decode base, stride and records, bounds-check, then the address-map load. Image (texture) paths unchanged. |
| buffer atomics | `InterlockedAdd` etc. on `RWByteAddressBuffer` from the heap at `e.copy`. Same semantics, since the copy is the UAV. |
| `emit/host.rs` root signature, `victory-gpu/src/pipeline/host.rs` | pull mode: root constants for the map index and 64-bit root view addresses; no root CBV/SRV descriptors. |
| `HostShader` | new fields: `pull: bool`, the root dwords for addresses. `shader_cache.rs` `encode`/`decode` serialise them, and **`MAGIC` goes to `VSH2`**, so old entries are refused rather than misread. |
| `emit/diff_tests.rs` | WARP diff tests: the same console program translated push and pull, comparing output buffers for aligned/misaligned CBs, dynamically indexed CBs, a buffer view with `num_records` smaller than the copy (out-of-range → 0), a page-crossing 16-byte load, and atomics. |

### 3.4 Shader cache key

- The switch is read **inside `victory-shader`** (`std::env::var("VICTORY_PULL_BUFFERS")`), so
  `build.rs` picks it up into `VICTORY_SHADER_SWITCHES`, and `translation_fingerprint()` mixes
  its value into the cache key [C]. Push and pull translations never share a cache entry. No
  manual key change is needed **as long as the literal string `"VICTORY_PULL_BUFFERS"` appears
  in the shader crate's source** (`build.rs` scans for quoted `VICTORY_*` literals).
- Pull mode changes the root signature, so the **pipeline library** must not hand back a
  push-mode pipeline. Pipelines are named by the translation, so a new fingerprint gives new
  names [C, `pipeline/library.rs` §1]. Check this in the first B2 run: the "pipelines: made /
  loaded" frame-log column should show a full rebuild.
- The switch is process-wide (no `alt`). Mixed push/pull pipelines in one run would read a
  root constant as a root descriptor. The first B2 run rebuilds every translation, so warm the
  cache (`victory translate --warm` with the switch set) before timing.

### 3.5 Hazards

1. **Bounds.** Push mode got bounds from the host view size. Pull must emit the console's
   rule, or a read past a buffer returns another buffer's bytes (silent picture errors; a
   write past it corrupts). Covered by diff tests and a `VICTORY_PULL_CHECK=1` mode: shaders
   also read through the old host view and count mismatches into a UAV counter, logged per
   frame.
2. **Address map staleness.** A shadow retired or remapped while a flight still uses the old
   map entry. Entries change only at submission boundaries, from a per-submission copy of the
   table (versioned ring, freed by the host fence like scratch).
3. **Descriptor bytes in flight.** The title rewrites a descriptor after recording the draw
   and before the GPU runs it. On the console the GPU reads it at run time too, so pull mode is
   *closer* to the console than push (push reads it at record time). The copy of the
   descriptor heap memory must be refreshed (it is a title buffer, dirty-driven in B2b).
4. **Card cost.** One dependent load per scalar address plus one per non-uniform address
   (L1/L2 hits; the map is small). Estimate +0.5 to +3 ms on ~46 ms of title card time [E],
   worse for programs that do per-lane addressing in loops. Mitigation: per-program census of
   uniform vs per-lane buffer addresses before B2; keep push for the heavy per-lane programs if
   needed (a per-pipeline mode bit, also in the key).
5. **Heap slots:** two views per copy in a reserved range. Copies number in the thousands,
   against a 1 M descriptor limit: fine. Sampler heap unchanged.
6. **Writes.** Shader writes land in copies as today. `Shadow::gpu_writes` declarations come
   from the UAV bindings the CPU decodes. In B2b these declarations still come from the
   console descriptors at bind (a write range is needed for copy-back), so **writable**
   bindings keep a cheap registration (no lookup lock: by address only, resolved at
   submission). Card-budget's modified-range lists (§3.1 there) use exactly these.

## 4. Stage B3: ExecuteIndirect runs

After B1 + B2, a draw's host state is: pipeline, targets, heaps (constant per list), root
constants (user data, table offsets, 64-bit buffer addresses), and the draw arguments.
ExecuteIndirect can set root constants and draw arguments per command, so:

- a **run** is consecutive draws (or dispatches) with the same pipeline, targets, topology,
  viewport/scissor and no barrier between them;
- a run records one `ExecuteIndirect(signature[root constants × N, draw], count)` over an
  argument buffer the recorder writes (upload memory, per list);
- this composes with recording-cost 4.4 (bind elision) and 4.5 (merging the title's own
  ExecuteIndirects), and shares the deferred-close lookahead.

The win on the CPU is the D3D12 calls (~15 ms of recorder CPU per present across threads [M])
and bind's per-draw path. Card cost: indirect command processing ~0.1-0.3 µs per command on
NVIDIA for root-constant changes [E]. The research's 20-40 ms for "run batching" assumed
translation-bound recording; here it is **1-2 ms per recorder thread [E]**. The real value of
B3 is that it makes **B4 possible**: a recorded list whose per-draw values live in an argument
buffer can run again with new values.

Switch: `VICTORY_DRAW_RUNS=1|alt` (runtime only; no translator change beyond B2's root
layout). Test: WARP test of a run of 3 draws with different root constants against 3 plain
draws, and an in-run picture check.

## 5. Stage B4: per-pass replay

A pass recorded on frame N-1 is executed again on frame N (`ExecuteCommandLists` of the kept
host list) when its hash at the right level matches, with that frame's values in its argument
buffer.

| hit level (tool, §7) | what a replay needs | available after |
|---|---|---|
| **exact** | nothing new in the list, but our `Deferred` steps (fills, refreshes, GPU-write declarations) must still run, and our in-list additions (indirect clamps) must read the same scratch | today, for passes with no fills (B1 makes fills rare) |
| **bound** | root constants, root view addresses and copy offsets from a per-frame record (argument buffer); the same descriptors, targets and counts | B2 + B3 |
| **shape** | also descriptor offsets and draw counts from the record | B2 + B3, with tables and counts as root constants per command |

Design points:

- **Key:** `(pass hash at the level used, pipeline set, target set, heap generation)`. The
  host list kept per key holds its allocator; up to 3 frames of copies per key (frames in
  flight), LRU-evicted by host-list memory.
- **Preludes:** the list's state transitions depend on states at submission, so they stay in
  the per-submission prelude list (as now) and are never in the replayed body.
- **Verification:** `VICTORY_REPLAY_CHECK=N` re-records one replayed pass in N, hashes **our
  host calls** (the same hasher as the tool, applied at our D3D12 call sites), and counts
  mismatches. Any mismatch disables replay for that key for the run.
- **Switch:** `VICTORY_REPLAY=bound|shape|exact` (`alt` supported).

**Build B4 only if the tool shows** (thresholds [E]):

- **≥ 60 %** of calls hit at the level B2/B3 deliver (bound), **and**
- **≥ 50 %** of calls are in heavy passes (≥ 64 calls), with **≥ 60 %** of heavy-pass calls
  hitting.

Otherwise replay saves recording of small passes whose per-pass overhead (key lookup,
argument fill, list bookkeeping) eats the saving. If **exact** is already high (≥ 50 % by
calls), try an exact-only replay first. It needs neither B2 nor B3 (stage R0,
`VICTORY_REPLAY=exact`), only care with `Deferred` steps.

Estimate [E]: at 60 % bound-level hits by calls, replay removes ~60 % of the recorders'
remaining per-draw CPU after B1-B3, ~4-8 ms per recorder thread at the target; at under 30 %,
~0. Research's "10-40 ms" was per frame in a translation-bound tree.

## 6. Ranked plan

| # | stage | switch | frame now [E] | recorders at target [E] | card [E] | effort | gate |
|--:|---|---|---:|---:|---:|---|---|
| 0 | **Pass-hash census** (built) | `VICTORY_PASS_HASH=1` / `barrier` | 0 | 0 | 0 | done | one run |
| 1 | B1 = port 5 stage A (CBVs in place) | `VICTORY_CBV_IN_PLACE=1` | 3-5 (submit side) | −2 to −4 / thread | ~0 | M | recording-cost §5 |
| 2 | B2a pull model, registrations kept | `VICTORY_PULL_BUFFERS=1` (in shader key) | ~0 | −1 to −2 | +0.5 to +3 | L (3-4 weeks: translator + map + tests) | diff tests; `VICTORY_PULL_CHECK=1` mismatch 0 |
| 3 | B2b dirty-driven refresh, no per-binding registration | `VICTORY_PULL_BUFFERS=2` | ±3 (sweep vs registrations) | −3 to −6 | 0 | M after submit-split stage 3 | write-watch rework landed |
| 4 | B3 ExecuteIndirect runs | `VICTORY_DRAW_RUNS=1\|alt` | 0 | −1 to −2 | +0 to +1 | M-L | after 4.4 bind elision |
| 5 | B4 replay | `VICTORY_REPLAY=exact\|bound\|shape` | 0 | 0 to −8 | 0 | L | tool thresholds §5 |

Sum at the target: about **−7 to −22 ms per recorder thread**, against ~31 ms per thread
today [M]. That is what lets recording fit under 33 ms once the submit thread is fixed. It
does nothing for today's frame.

## 7. The measuring tool: `VICTORY_PASS_HASH` (built)

Branch `pass-hash`, `crates/victory-gpu/src/passhash.rs`. Diagnostics builds only; off
unless set.

- `VICTORY_PASS_HASH=1`: passes end at a render-target change, a wait for a memory value, and
  the list's close. `VICTORY_PASS_HASH=barrier`: also at every barrier and barrier group.
- Every title list call (draws, dispatches, indirect, state, tables, constants, views, vertex
  and index buffers, targets, barriers, copies, clears, queries, waits, writes: 46 call
  sites) mixes its arguments into three running hashes of its pass:
  - **shape:** command kinds, pipelines, root slots, heaps, barrier kinds;
  - **bound:** shape plus descriptor handles, targets, resources, viewports, draw and
    dispatch counts;
  - **exact:** bound plus root constant values, root view and vertex/index addresses, copy
    offsets, clear colours.
- At `ExecuteCommandLists`, the title's closed passes join the frame in submission order. At
  present the frame's passes are matched as **multisets** against the frame before, and
  against either of the two before (for ping-pong targets).
- One line per frame:

```
pass hash: frame N: P passes, C calls (W make card work), L lists (R run again unreset);
compute lists Pc passes Cc calls; hit vs last frame by passes exact/bound/shape %, by calls
exact/bound/shape %; vs either of the last two by calls ...; heavy passes (>= 64 calls) X % of
calls, hit by calls ...; run so far by calls exact/bound/shape %
```

- The **"run so far by calls"** figures at the end of the run are the answer. Read the
  in-world frames, not the menu (the menu is likely near 100 %).
- Cost: a few multiplies and an uncontended per-list mutex per call. Draws ~40 k calls a frame.
  Expect well under 1 ms of recorder CPU a frame [E]. The per-frame compare clones a few
  thousand-entry maps on the present thread (<0.5 ms [E]). It is a census, so run it on its
  own, not with timing runs.
- The same build also adds a **card meter line** (no switch; on whenever the card meter is):
  every 256 frames, the card time of the title's compute-type lists, for async compute
  (`async-compute.md`).

Run: one diagnostics build with `VICTORY_PASS_HASH=1`, 600 s into the world. Optionally a
second run with `=barrier` if the first shows few, large passes.
