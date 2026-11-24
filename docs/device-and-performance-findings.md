# Device removal, performance and translation gaps — measurements

Findings from a local session on an RTX 4070 Ti (driver 610.88) / i7-10700KF /
Windows 11 26300, against the retail Series 1.0.2.1 package, `main` `d2a4bae`
plus the local branches below. Every number here is from a real run.

## 1. The device removal is fixed

The debug layer (Graphics Tools enabled) named it exactly:

```
ID3D12Device::CreateRenderTargetView: The Format (0x56, B5G5R5A1_UNORM) is
invalid, when creating a View; it is not a fully qualified Format castable from
the Format of the Resource (0x35, R16_TYPELESS).
```

The packed 16-bit console formats are emulated as `R16_TYPELESS` copies, so a
view of them must use a fully qualified format of that family. Views, the
pipeline state's `RTVFormats` and the resource's clear value now all name the
same format (`host::view_format`, `Host::render_format`). After this the world
runs with **zero device removals**; before it, every run died at frame ~1519.

Also fixed on the way, each verified:

- **VRAM exhaustion.** Host textures are never released; the world streams them
  to **13,561 MiB** on a 12 GB card, which removes the device with no invalid
  call anywhere. Descriptor slots now release with their texture,
  `host::HostVideoMegabytes` scales the title's graphics budget to the card, and
  the running total is logged at each power of two.
- **Null command lists.** `ExecuteCommandLists` with a `None` list is an invalid
  call that removes the device; failed casts are filtered out.
- **Release builds have no diagnostics.** `VICTORY_SKIP_PIPELINES` and its hash
  parser were behind `feature = "diagnostics"`, so every release-run skip test
  silently skipped nothing. Both now work in release.
- `TdrDelay`/`TdrDdiDelay` 60 on hosts too slow for the 2 s watchdog (the game
  then dies in its *own* watchdog instead — `0xC0000005` at `+0x23cf683`).

## 2. Where the frame time goes

CPU-side work is negligible: their own copy statistics report **0.2–0.3 ms** of
copies, refreshes and watches per frame against a 450–1,300 ms frame. The cost
is GPU instructions.

Per-command GPU timings (`VICTORY_GPU_TIMING`, `VICTORY_GPU_SLOWLOG`) name the
shaders, once a pipeline hash is mapped to its container:

| pipeline hash | container | cost per workgroup |
|---|---|---|
| `04b9be3756e8f41c` | `MS_VS_Transform_eb34a03f` | 305 ms |
| `5a801f1f568c7abd` | `MS_VS_Transform_eb34a03f` | 341 ms |
| `c5ffa556ab926155` | `MSNC_VS_TransformSkin_26a320a2` | 265 ms |
| `a26422c01dcf5612` | `MSNC_VS_PedTransformSkinAlpha_af24a1cd` | 228 ms |

`shader-info` now prints each container's pipeline hash (first 16 hex of
blake3 over the container bytes), which is how the mapping above was made.

**These shaders contain no loops** (`MS_VS_Transform`: 617 instructions, zero
backward branches), so the cost is not loop iteration. Opcode histogram
(`VICTORY_OPCODE_STATS=<hash>`), per lane:

```
tertiary.f32 302  rawBufferLoad.f32 128  unary.f32 90  annotateHandle 69
createHandleFromHeap 52  binary.f32 34  storeVertexOutput.f32 34
waveGetLaneIndex 26  waveReadLaneAt.i32 24  legacyF16ToF32 22
```

Resolution barely matters (3840x2160 -> 1280x720 changed little) because the
cost is per workgroup, not per pixel. Frame time scales with
`VICTORY_SHADER_LOOP_LIMIT` (4096 -> 966 ms, 64 -> 477 ms) but the limit path
discards pixels, so lowering it blackens the screen.

## 3. Translation gaps are chains, not single causes

`victory translate` over `shaders.rpf` (26,084 programs): **12,272/14,001**
vertex+pixel, **1,693/2,581** compute, **8,191/8,506** mesh emit.

Fixing `image_sample_d_cl` (27 shaders) moved those 27 to `an image address`
and left the emitted totals identical. The failure total stayed at **3,733**
before and after. Each failing shader has several limitations, so clearing one
moves the boundary without emitting anything.

The six systemic limitations behind the 3,733:

| cause | count | what it needs |
|---|---|---|
| 64-bit image atomic | 653 | `R64_UINT` views or CAS pairs |
| scalar memory read as data | 573 | a model of console scalar memory, and where its SGPR addresses alias |
| descriptor neither loaded nor built | 298 | descriptor provenance |
| select on an untracked mask | 288 | mask tracking through control flow |
| lane mask holding lanes elsewhere, read as data | 276 | ballot/mask emulation across sections |
| lane mask read as data | 206 | same machinery |

## 4. Switches added for this work

All env-gated and off by default:

- `VICTORY_GPU_TIMING`, `VICTORY_GPU_SLOWLOG` — per-command GPU time
- `VICTORY_OPCODE_STATS[=<hash>]` — emitted instructions by name
- `VICTORY_CUT_AT`, `VICTORY_CUT_FROM` — bisect a removal to one command
- `VICTORY_DROP_DRAWS`, `VICTORY_DROP_OPS`, `VICTORY_DROP_PRELUDE`
- `VICTORY_CHUNK` — submit and wait every N commands
- `VICTORY_ASYNC` — do not wait for each submission
- `VICTORY_TEXTURE_BUDGET_MIB` — cap host texture memory

## 5. What to do next

1. Take **one** shader from the 573 (`VS_TransformSkin_VfxVBuffer_ac5733c2`)
   and build the scalar-memory model against it end to end; only generalise
   once one shader emits *and* validates.
2. Instruction-level cost lives in the per-lane math, not bookkeeping:
   an entry-block handle hoist (implemented here) saved only 61 -> 52 handle
   instructions. The wins to chase are f16 fast paths, vectorised
   `rawBufferLoad`, and the 116 refused wave64 compute shaders.

## 6. Dropped compute passes: the wave-size declaration

The world run logs a burst of `host pipeline refused error=The parameter is
incorrect. (0x80070057)` for **203** compute pipelines that translated fine, and
the title writes to null (`0xc0000005`, `+0x23cf683`) immediately afterwards.

Cause: the translator declared `WaveSize(64)` for those programs, and a 32-lane
device cannot create a 64-lane pipeline at all — D3D12 returns `E_INVALIDARG` and
the pass is dropped silently.

Two measurements pin it down:

| run | compute refusals | reaches world | crash |
|---|---|---|---|
| before | 203 | yes | yes |
| declare only for lane readers (`27fe189`) | 115 | yes (1,224 draws) | yes |
| `VICTORY_NO_WAVESIZE=1` (nothing declared) | 1 | yes | **no** |

So the crash is the title reading data that a dropped pass never wrote, not the
renderer.

### What the remaining 115 need

`console_lanes` is set by `ds_swizzle*`, `permute*`, `v_mbcnt*`,
`v_readlane/writelane_b32`, DPP, or a lane mask read as data. All of those
assume the shader knows it is lane 32..63; a 64-thread group on a 32-lane device
is two waves.

Emulation, in the order it should be built:

1. **Lane identity.** `waveGetLaneIndex` becomes
   `group_thread_index / 32 * 32 + WaveGetLaneIndex`; `waveGetLaneCount` is the
   console's 64, not the device's 32.
2. **Wave-uniform values across the pair.** `waveReadLaneFirst` and friends
   currently rely on device wave scope; they need an LDS slot plus a group
   barrier to read the other half-wave's value.
3. **Masks.** A 64-bit lane mask becomes two 32-bit device ballots joined by the
   half-wave bit, so `v_cmp` results and `v_mbcnt` count across the pair.
4. **LDS for one wave.** The console compiler omits barriers for LDS shared
   inside one 64-lane wave; two device waves need the barrier the console left
   out.

Acceptance: compute refusals reach 0, the null-write crash stops happening, and
the world renders. `VICTORY_NO_WAVESIZE=1` is the diagnostic that shows the
target state (1 refusal, no crash, 30 fps in menus at full quality).
