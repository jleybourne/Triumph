# DLSS Super Resolution in place of the title's FSR: design

Status: design only, nothing built or measured. Items marked VERIFY are inferences from the
FidelityFX API shape and the X reference layer's notes, not things read from our own logs.

## 0. What we know

- The title imports `amd_fidelityfx_loader_xbox.dll`; our loader maps it as a title DLL
  (`apps/victory/src/run.rs:76`) and binds its imports ("4 hle, 18 host"). Those are the DLL's
  imports, not its exports.
- the X reference layer's notes (`gpu/recomp/pso.rs`, `hle/d3d12x/*`) say the console upscaler is a second DLL,
  `amd_fidelityfx_upscaler_xbox.dll` (FSR4), which the loader front-ends. The upscaler builds its
  passes from console containers (an `ACCC` part holding `FSPX`: RDNA2 code, no DXIL) through
  device method 0x218 (`unknown_pipeline::<0x218>`, `crates/victory-gpu/src/device.rs:441`; the real
  version is on branch `fsr`, `4b3fe7e2`, behind `VICTORY_PRECOMPILED`). It also issues console
  list calls (UAV clears, working-set queries).
- Title: 2560x1440 scene, 3840x2160 present. `fsrQuality` 3 and 4 give the same scene size.
- Measured: `VICTORY_PRECOMPILED` costs 0.115 ms/draw vs 0.029 from the library. The frame is
  bound by memory mirroring (submit thread 74 ms), not the card (CLAUDE.md "Where a frame goes").

## 1. Where to intercept

Three candidates, in order of preference.

A. **The loader's exports (recommended).** The loader DLL is the FFX API front door. Its PC
   counterpart exports five entry points: `ffxCreateContext`, `ffxDestroyContext`, `ffxConfigure`,
   `ffxQuery`, `ffxDispatch` (VERIFY the console loader has the same set: dump its export table
   with the PE reader in `crates/victory-loader`). Replace those exports with our own HLE
   functions (a new module `crates/victory-hle/src/modules/ffx.rs`, registered through
   `crate::registry` the way other modules are) so the upscaler DLL is never entered. Benefits:
   - The dispatch descriptor already holds every input as a named field (below), so there is no
     reverse engineering of passes.
   - We skip 0x218 entirely: no FSPX pipelines, no 4x per-draw cost, the console-pipeline blocker
     (precompiled assert at startup, blocker 0) is bypassed on the DLSS path.
   - Failure is contained: with the plugin absent we implement the same exports with a fallback
     (section 5), so the title never sees a missing upscaler.
   Cost: the descriptor structs must be decoded from title memory. The layout is the public
   FidelityFX API (`ffxApiHeader` linked list: type, pNext; `ffxDispatchDescUpscale`). The console
   build may differ in pointer width/packing, so first log one create and one dispatch (header
   types, struct sizes) before writing the decoder. This is the first checkpoint.

B. **Intercept the dispatches (the 0x218-built compute passes).** Too late: by then the inputs are
   spread across ~10 FSR4 compute passes with internal intermediates, and motion vectors/jitter are
   constants in a cbuffer. Not recommended; only useful for diagnosing.

C. **Hook inside the upscaler DLL** (find its internal dispatch function). Fragile across title
   updates and gains nothing over A. Only if the loader turns out to export a single opaque
   function table.

Design A, with a log-only passthrough first (call the real export, log the descriptor), so the
title keeps running while we learn the layout.

## 2. Getting the inputs

All of these come from `ffxDispatchDescUpscale` (names from the public API; VERIFY each against the
logged descriptor):

| DLSS input | FFX field | Notes |
|---|---|---|
| colour (in) | `color` | FfxApiResource: pointer + desc + state |
| depth | `depth` | reversed-Z and infinite-far likely; set the DLSS depth-inverted flag from `cameraFar`/`cameraNear` (flags bit `ENABLE_DEPTH_INVERTED`, `ENABLE_DEPTH_INFINITE`) |
| motion vectors | `motionVectors` + `motionVectorScale` | FFX takes MVs in render-res pixels times a scale; DLSS takes `MVScaleX/Y` the same way, pass through. Low-res MVs are the DLSS default |
| jitter | `jitterOffset` | pixels, same sign convention question (VERIFY by A/B screenshot; DLSS expects jitter in pixels, FSR's is also pixels but check sign) |
| exposure | `exposure` (+ `preExposure`) | pass the texture as DLSS exposure input; if null set `DLSS_FEATURE_FLAG_AUTO_EXPOSURE` |
| reactive mask | `reactive`, `transparencyAndComposition` | DLSS Super Resolution has no direct reactive input. Options: drop (simplest), or bias via `DLSS bias current color mask` (`pInBiasCurrentColorMask`) fed from the reactive texture |
| output | `output` | 3840x2160 UAV-capable texture owned by the title |
| sizes | `renderSize`, `upscaleSize` | DLSS `InRenderSubrectDimensions`, output dims |
| delta time, reset | `frameTimeDelta`, `reset` flag | `InReset`, frame time in ms |
| camera | `cameraNear/Far/FovAngleVertical` | informational for DLSS; sharpness is deprecated, leave 0 |

Resource mapping: `FfxApiResource.resource` is the title's console D3D12 resource object. Our
object model resolves it to a host `ID3D12Resource` (see `crates/victory-gpu/src/object.rs` and
`resource.rs`; the host handle lookup used by `host.rs` for draws). VERIFY three things:
1. Resources are created with the flags DLSS needs. Our `create_committed`/`create_placed` in
   `resource.rs` must allow UAV on the output and the colour/depth be plain textures. If the
   title's heap-placed resources are aliased (placed on shared memory), DLSS needs them in the
   right state, so insert barriers on the host list (via `host::transition`).
2. Depth format: the title's depth may be a typeless D32 or D24S8; DLSS needs a readable SRV state.
3. The copy model: with the memory mirror (`shadow.rs`) active, GPU-written textures live on the
   host only. Colour/depth/MV are card-side outputs, so no copy is needed, but a pending hash pass
   or copy-back must not race the DLSS dispatch. Record DLSS on the same host list at the point
   where the title dispatches, so ordering follows the title's list.

Command list: the descriptor carries the title's command list. Map it to our `HostList` (the thing
`draw.rs`/`indirect.rs` record into) and record the DLSS evaluate there, ending with the output in the
state the title expects (FFX leaves `output` in UAV/`PIXEL_SHADER_RESOURCE`: VERIFY by reading the
`state` field and the title's following barrier).

## 3. Driving DLSS

Two routes. Recommend NGX (direct) for a small, controlled integration; Streamline if we later want
Frame Generation (plan item 5 in CLAUDE.md) since it also manages DLSS-G and the interposer.

NGX (`nvngx_dlss.dll` + `nvsdk_ngx_d.dll` or the static `nvsdk_ngx_d.lib`, loaded at runtime):
1. At `ffxCreateContext`: `NVSDK_NGX_D3D12_Init_with_ProjectID` (our own project id; engine type
   custom) on the host device, `NVSDK_NGX_D3D12_GetCapabilityParameters`, check
   `SuperSampling.Available` / `NeedsUpdatedDriver`.
2. `NVSDK_NGX_D3D12_CreateFeature(cmdList, NVSDK_NGX_Feature_SuperSampling, params)` with
   `Width/Height` = render size, `OutWidth/OutHeight` = 3840x2160, `PerfQualityValue` mapped from
   `fsrQuality`, flags: `MVLowRes`, `IsHDR` (the title's colour is likely HDR scene-referred linear:
   VERIFY), `DepthInverted`, `AutoExposure` if no exposure texture. Feature creation must be
   recorded on a list and executed; do it on a dedicated list at startup, not mid-frame.
3. At `ffxDispatch` upscale: `NVSDK_NGX_D3D12_EvaluateFeature_C` on the host list with the inputs above.
4. `ffxDestroyContext`: `ReleaseFeature`, `Shutdown1`.

Streamline alternative: `slInit`, `slSetD3DDevice`, `slSetTag` for each buffer,
`slSetConstants` (jitter, MV scale, near/far, reset), `slEvaluateFeature(kFeatureDLSS)`.
More moving parts (interposer must wrap the device and swapchain creation, which our host layer
owns), so only worth it for Frame Generation.

DLSS requires the host device to be created where NGX can see it (it is: the host D3D12 device in
`host.rs`). Ray-tracing/DXR is not needed. Wave size is irrelevant to DLSS.

## 4. Lower internal resolution

Today the scene stays 2560x1440 at `fsrQuality` 3 and 4 alike, and reporting a smaller display mode
did not change the GPU time (CLAUDE.md blocker 2). That implies the title sizes its targets from its
own settings, possibly through the FFX API. Candidates, in order:

1. **The loader's `ffxQuery`.** The PC API has an upscale query for the render resolution from a
   quality mode (and a ratio query). If the title calls it to size its targets, answer it from our
   HLE with our chosen scale. Log all `ffxQuery` header types to find out. The strongest lever if it
   applies, and it is the same place we already intercept.
2. **Settings.** The title selects the ratio from `fsrQuality` in settings.xml. Level 3 giving 1.5x
   (2560x1440 -> 3840x2160 is exactly 1.5) suggests levels map to Quality/Balanced/Performance/
   Ultra Perf; level 4 giving the same size suggests the title clamps or ignores it. Test
   `fsrQuality` 5 and 6 only as single-variable runs, one setting per run, screenshot each.
3. **Resource creation override.** In `resource.rs::create_committed` scale the width/height of
   targets matching the render-size signature (11 targets at 2560x1440, 1280x720 and 960x540 effect
   buffers). Dangerous: the title writes viewport, scissor, UAV dispatch counts and constants from
   its own numbers, so a mismatch corrupts output. Last resort.

With DLSS, set `renderSize` to the scale the quality mode yields. Standard DLSS ratios: Quality 1.5,
Balanced 1.72, Performance 2.0, Ultra Performance 3.0. 4K Performance = 1920x1080.

## 5. Fallback when the GPU is not NVIDIA (or the plugin is absent)

Decide at `ffxCreateContext` from the DXGI adapter vendor id (0x10DE) and NGX capability:
- NVIDIA with DLSS available: DLSS path.
- Otherwise: Fallback 1 (cheap, always available): a simple in-tree upscaler in our own code,
  a bilinear/Catmull-Rom resample plus optional RCAS-like sharpen, in `crates/victory-gpu`
  (new `upscale.rs`), writing the 4K output from the render-size colour. No temporal data needed.
- Fallback 2 (better on AMD/Intel): hand back to the title's real FSR4 path (the `fsr` branch, 0x218
  precompiled) or a portable FSR1/EASU+RCAS reimplementation. FSR1 is MIT-licensed upstream; writing
  our own EASU/RCAS from the public algorithm keeps us clean either way.
- Intel: XeSS is a possible later plugin with the same dispatch descriptor mapping.
The selection is one enum behind the HLE export so all vendors share the same descriptor decoder.

## 6. Licensing (needs the owner's decision; not legal advice)

Facts: our repo is GPL-3.0. The NVIDIA DLSS SDK (NGX headers, `nvsdk_ngx` libs, `nvngx_dlss.dll`)
is proprietary: its licence permits redistribution only of the runtime DLLs under NVIDIA's terms
and forbids modification; the headers are under NVIDIA's SDK licence, not an open-source one.
GPL-3.0's "System Library" exception and its anti-linking rules mean that statically or
dynamically linking a GPL program with a closed library, and then distributing the result, is
widely considered a problem when the library is not a standard component of the operating system;
NVIDIA's driver-supplied pieces are the usual argument for an exception, but the SDK DLL is not
the driver. Options:

1. **Optional runtime-loaded plugin in a separate repository/binary.** The GPL core defines a small
   C ABI ("upscaler plugin": init/create/evaluate/destroy, plain structs of D3D12 pointers) and
   loads `victory_dlss.dll` via `LoadLibrary` if present. The plugin links NGX, is distributed
   separately under its own licence, and is not part of our GPL combined work. This is the clearest
   separation but still arguable, since the plugin is designed to run inside the GPL process. Mitigation:
   a generic interface that other vendors can implement.
2. **User-supplied DLL.** We ship no NVIDIA binaries or headers; the user copies `nvngx_dlss.dll`
   (from a game or the NVIDIA SDK download) next to the exe, and we resolve the NGX entry points
   at runtime by name (`GetProcAddress` on `nvsdk_ngx_d.dll`), with our own declarations of the
   needed structs written from the public API. Risk: re-declaring SDK headers may itself raise copyright and licensing questions;
   binding only by name and carefully minimal structs lowers that. NVIDIA's EULA also bars
   redistribution of the SDK by anyone but NVIDIA's licensees.
3. **Ship the closed plugin with the GPL core in one package.** Highest risk (aggregation vs
   combined work); do not do this without a lawyer.
4. **Relicense or add a linking exception** for the NVIDIA module (as some GPL projects do for
   OpenSSL). Needs agreement of all copyright holders, and since upstream forks (the X reference layer is GPL-2.0-only)
   have contributed ideas only, that matters mainly for any copied code (none per CLAUDE.md).
5. **Do not ship DLSS**; offer only vendor-neutral upscaling.

Whichever applies: DLSS also has a branding rule (logo/attribution) and a requirement to register a
project id; and the title's own EULA is a separate matter. Owner must choose between 1, 2 and 5
before any public release; private use is outside the question.

## 7. Plan with checkpoints

Preconditions: the playable build (`pipeline-samplers` `d1e1af34`), diagnostics build, 600 s runs,
one setting per measurement, report `fps · ms · draws · dropped` (CLAUDE.md rules).

1. **Reconnaissance (no behaviour change).** Dump the loader's export table; wrap each export with
   a logging passthrough in `ffx.rs`. CP1: log shows the create/configure/query/dispatch calls,
   header types, struct sizes, render and upscale sizes, resource pointers and descriptions for one
   frame. Exit if the loader has no usable exports (then fall to option C).
2. **Descriptor decoder + resource lookup.** Decode `ffxDispatchDescUpscale`, map the command list and
   six resources to host objects, log formats/states. CP2: every input resolves to a host
   resource with plausible formats; a capture (`capture.rs`) of colour/depth/MV looks right.
3. **Replace with a trivial output.** Make `ffxDispatch` do a bilinear copy render-size -> output
   (the section 5 fallback). CP3: the title shows a correct picture without running FSR passes and
   without 0x218 (draw count unchanged or higher; this also measures FSR's current cost, section 8).
4. **NGX behind the plugin interface.** Implement option 1 or 2 from section 6 (decision gate).
   CP4: DLSS output renders, with correct jitter/MV signs (A/B stills, no ghosting on pans), correct
   depth inversion; `reset` on cuts.
5. **Quality knobs and masks.** Map `fsrQuality` to DLSS modes, add reactive via bias mask if needed,
   handle HDR/exposure. CP5: matching or better image than FSR at the same level, screenshot compare.
6. **Lower internal resolution.** Answer `ffxQuery` (or settings) for 1920x1080 / 1707x960. CP6:
   target sizes in the log change and the `passtime` figures drop; draw counts equal.
7. **Fallbacks and gating.** Vendor/capability detection, graceful absence. CP7: runs on a non-NVIDIA
   path (or forced via an env switch), no crash with the DLL missing.
8. **Launcher.** Only after a run that beats the baseline at the same draw count does it enter
   `run_play.ps1`.

## 8. Expected GPU-time effect (assumptions)

Assumptions: the title's GPU busy is ~45 ms in its own lists plus ~26 ms of our lists (CLAUDE.md);
FSR passes are not currently separated (use `passtime.rs` to read them at CP3). DLSS Quality at
2560x1440 -> 3840x2160 on a 4070 Ti costs about 0.8-1.5 ms (assumed from public benchmarks; not measured
here); FSR4 through our translated wave64-on-wave32 path is probably several ms more, and the
0x218 path costs 4x per draw. Expected: removing the translated FSR passes saves a few ms of card
time (assume 2-6 ms), and a 1920x1080 internal resolution (Performance) cuts the scene's pixel
work by ~44% versus 2560x1440, which on pixel-bound passes could save 10-20 ms of the ~45 ms title
GPU time (assumption; effect buffers at 1280x720/960x540 would not shrink unless they derive from
the render size).

**Important:** the frame is bound by the submit thread (74 ms) and present wait (67.5 ms), with
the card idle 43.8 ms. Any GPU saving appears in fps only once the memory-mirroring wall comes
down (UMA, THIN). Until then DLSS is expected to change frame time by roughly 0 and show up only in
the card-busy figure and in image quality. This agrees with CLAUDE.md's step order: GPU work is third.

## 9. Files and functions to touch

- New `crates/victory-hle/src/modules/ffx.rs`: `ffx_create_context`, `ffx_destroy_context`,
  `ffx_configure`, `ffx_query`, `ffx_dispatch`; descriptor decode structs; register in
  `modules/mod.rs` and make the loader override the DLL's exports (`crates/victory-loader`, near
  where `imports bound` is logged; `apps/victory/src/run.rs`).
- New `crates/victory-gpu/src/upscale.rs`: `trait Upscaler { create, dispatch, destroy }`,
  `DlssUpscaler` (plugin facade), `BilinearUpscaler` (fallback), `select(adapter)`.
- `crates/victory-gpu/src/object.rs` / `resource.rs`: a public `host_resource(This) -> Option<ID3D12Resource>`
  and the flags/state accessors; `host.rs`: `host_list(This)` and `transition` use.
- `crates/victory-gpu/src/device.rs`: leave `unknown_pipeline::<0x218>` in place; with the HLE export the
  DLSS path never reaches it. The `fsr` branch stays as fallback 2.
- Config: a `[upscaler]` section in `victory.toml` (mode, plugin path, scale override).
- `crates/victory-gpu/src/passtime.rs`: add an upscale pass bucket for CP3/CP6 measurements.

Open questions for the owner: licensing option (section 6); whether the loader's exports match the
PC FFX API (CP1 answers it); whether to keep FSR4 via 0x218 as the AMD path.
