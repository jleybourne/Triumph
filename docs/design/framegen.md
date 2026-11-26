# Frame interpolation in our layer: open-source FSR 3 frame generation, with an optional user-supplied DLSS frame generation (design)

Status: design only. Nothing built, nothing vendored, no game run. Facts marked MEASURED come from
our own FSR/draw traces (`VICTORY_FSR_TRACE=1`, `crates/victory-gpu/src/fsrhook.rs`, runs
`logs/victory-run-20261127-004114`, `-024737`, `-032053`, `-033324`). Facts about the FidelityFX SDK
and Streamline are from memory and marked (unverified) until read from the source at the pinned tag.
Builds on `research-framegen.md` items 1.1, 1.2 and 3, and on the steady presenter
(`VICTORY_STEADY_PRESENT`, branch `steady-present`), which is the single owner of `Present` that frame
generation needs.

## 0. What we have (MEASURED unless said)

| Input | What and where | Status |
|---|---|---|
| Presented frame | 1 plane, 3840x2160, format 28 (R8G8B8A8_UNORM family); every `VICTORY_UI_PLANES=log` run says `planes=1`. The HUD is baked into plane 0 (`captures/targets.png` shows the control-hint box in plane 0). | measured |
| Scene colour, pre-HUD (world) | Draw `0309a446e1a48352+4f4ec7b16673362f` is the 1440p to 2160p upscale in the world. It reads `s1+0` 2560x1440 f24 (R10G10B10A2_UNORM) colour and `s0+168` 64x64 f56 (a small lookup texture), and writes `rt0` 3840x2160 f28. It writes no depth target. The `rt0` it writes alternates between two objects (`0x1a5e47efb10` 23 times, `0x1a5e47f0810` 27 times in the traced frames): a double-buffered output. The hook's `fsr::plan_draw` already resolves its inputs (`plan=Ok`: colour `0x1a6eba486d0` f24, depth `0x1a5e47ef3c0` 2560x1440 f21, motion `0x1a6eba4b860` 2560x1440 f34). | measured (run 16, `logs/victory-run-20261127-033324.log.err`, frames 2407-4800, 50 trace lines) |
| Loading-screen composite (not the world) | Draw `ea212fca1f658aef+1579833b3d76cfb1` runs only while loading (traced frames 1430-1434 in run 16; ~1,428-1,446 in run 15). It reads 2560x1440 f28 colour plus depth and writes `rt0` 3840x2160 f28 (one object, `0x1a6eba46240`) plus a 3840x2160 f19 depth (`ds`). Its cb `s0+24` holds 3840.0, 2160.0, 2560.0, 1440.0 as floats. Its trace line ends in `plan=Err("no motion vectors read lately …")`. Frame generation must never take its frames. | measured |
| Depth | 2560x1440 f21 (R32_FLOAT_X8X24, a D32S8 depth). In the world, the same object (`0x1a5e47ef3c0`) is read by dispatch `11bbed94ee7a3243`, together with the vectors, and is the depth `plan_draw` pairs with the world upscale. | measured |
| Motion vectors | 2560x1440 f34 (R16G16_FLOAT), read by `11bbed94ee7a3243` (with depth, writes R32_UINT 2560x1440) and by `559fb027c6d5b1db` (a temporal pass at 2560x1440 that also reads 1280x720 HDR colour). Render-resolution vectors. | measured; scale/sign unverified |
| Projection | A row-vector 4x4 found 190-380 times per run in constant buffers of many passes (`3811b0734e389cd6` cb s1+16 word 16, `5fae02b711504eee` s1+56, `9938c251a313fe28` s1+40, ...): m00 1.26578, m11 2.25027 (aspect 1.778), m23 -1, m33 0, m20/m21 small (0.00022, 2e-05: the jitter), and two forms: m22 ~0 with m32 +0.05 (reversed-Z, infinite far, near 0.05) and m22 -1.00001 with m32 -0.05 (forward-Z, infinite far). So: vertical FOV 2·atan(1/2.25027) = 47.9°, horizontal 76.6°, near 0.05, far infinite, reversed depth for the main view. | measured (scan of trace constants); which form the depth buffer uses is inferred (reversed: m22≈0) |
| Frame time | Our own: the steady presenter's real-frame intervals (`crate::steady`). | built on `steady-present` |

Not measured yet:
- which draws write the HUD into `rt0` after the world upscale `0309a446…`;
- whether the `rt0` of that upscale is the texture the title presents. It is likely: same size and
  format, and it alternates between two objects as a presented pair would. But the trace does not log
  the present's object;
- the motion vectors' scale and sign;
- whether the vectors include camera motion. They are read with depth by the TAA-like pass, so
  probably full per-pixel motion.

## 1. Shape of the solution

```
title's frame n ──► [extract] depth, MV (1440p), HUD-less colour (2160p copy), camera  ──► FgInputs(n)
                      │                                                                       │
                      └► presented colour with HUD (2160p, plane 0) ──► mailbox(n) ────────────┤
                                                                                              ▼
steady presenter tick ──► provider.generate(FgInputs(n-1), FgInputs(n)) ──► interpolated(n-½) ──► Present
                       └► at the next tick: real(n) ──► Present
```

- One owner of `Present`: the steady presenter thread (`crate::steady`). Neither the FidelityFX frame
  interpolation swap chain nor Streamline's swap-chain proxy is used. The research doc warns their swap
  chains fight any presenter we own; we keep ours and only take the interpolation work.
- One input extractor (sections 2-4), shared by both providers. The providers stay small.
- Frame generation needs the steady presenter in place: interpolated frames go out on its ticks. With
  a 30 Hz cadence and 15 real fps, each tick alternates generated and real (2x). At 20 real fps and
  a 40 Hz cadence it is also 2x. Real fps not dividing the cadence is handled by the floor lock
  (`VICTORY_STEADY_FLOOR=auto` already locks real frames to cadence/k).

## 2. HUD-less colour

The title presents one plane with the HUD drawn in, so the HUD-less colour has to be copied out at the
right moment in its command stream.

Where: right after the world upscale draw `0309a446e1a48352+4f4ec7b16673362f` writes `rt0` 3840x2160,
and before the first HUD draw writes the same `rt0`. That draw is the last 3D-to-output pass in the
world. We record `CopyResource(rt0 → fg_hudless[n%2])` in the title's own list after that draw, so it
runs in order on the card at almost no cost (one 33 MB copy, about 0.05 ms at 2160p). `rt0` is one of
two alternating objects, so the copy takes whichever object this frame's draw wrote, not a remembered
one. The loading-screen composite `ea212fca1f658aef+1579833b3d76cfb1` is never a copy point.

How it is found, without a per-frame hash list:
1. Signature: a draw that writes a colour render target at the presented size (3840x2160, format
   family 28) and samples a colour texture at the render size (2560x1440). Also, `fsr::plan_draw` must
   succeed for it: it must pair the draw with a depth and motion vectors read lately at the render size,
   as the trace's `plan=Ok(…)` shows for `0309a446…`. Requiring the vectors is what excludes the loading
   composite, which ends `plan=Err("no motion vectors …")`.
2. Confirmation: the texture it writes is the one the frame presents. `queue::present_now` sees the
   present object; compare there and count the matches. A frame where they differ (menus, video,
   loading) gets no HUD-less copy, and frame generation is off for it (reset).
3. Override: `VICTORY_FG_HUDLESS=<hash>` names the draw once a trace has shown it, as
   `VICTORY_FSR_HASHES` does for DLSS. The default is `0309a446e1a48352` (pixel and vertex hash pair as
   traced).

Telling the loading pass from the world pass (stage 0 logs every test below for each candidate and
says which one it chose):
- motion vectors: the world upscale has a successful `plan_draw` with 2560x1440 f34 vectors; the
  loading composite has none (`plan=Err(…motion vectors…)`). This is the primary test;
- shape: the world upscale reads f24 colour and the 64x64 f56 texture, and writes no depth; the loading
  composite reads f28 colour and writes a 3840x2160 f19 `ds`;
- output: the world `rt0` alternates between two objects frame to frame; the loading composite's is a
  single object;
- recurrence: the world draw is seen every frame for thousands of frames, the loading draw only for a
  few dozen frames around a load. Generation starts only after 8 consecutive frames whose candidate
  passes the tests, and resets on any frame without one.

Fallback when no composite is found in a frame: generation is skipped (repeat), never interpolate a
frame with the HUD baked in (UI warping is the worst artefact).

UI handling with HUD-less colour: FSR 3 frame generation (unverified, FSR 3.1 API) can take both the
HUD-less colour and the final presented colour and treats the difference as UI, composited unwarped on
the interpolated frame ("HUD-less" mode). That is exactly our case (we have both images). DLSS frame
generation takes the same pair (`kBufferTypeHUDLessColor` plus the backbuffer). No UI-only texture is
needed. Alternative if the difference mask is poor on bright effects: copy `rt0` before and after the
HUD draws and build a UI colour+alpha in our own compute pass (later, only if needed).

## 3. Camera data from constant buffers

Needed by FSR 3 frame generation (unverified field names, FSR 3.1 `ffxDispatchDescFrameGenerationPrepare`):
`cameraNear`, `cameraFar`, `cameraFovAngleVertical`, `viewSpaceToMetersFactor`, `frameTimeDelta`,
`jitterOffset`, `motionVectorScale`, `renderSize`, flags for reversed depth and infinite far. DLSS
frame generation additionally wants `cameraViewToClip`, `clipToCameraView`, `clipToPrevClip`,
`prevClipToClip`, camera position/axes (Streamline `sl::Constants`, unverified), which we can derive
from the same projection plus a view matrix found the same way.

Plan:
- A pure matcher `fg::camera::find_projection(words) -> Option<Projection>` over the constant words
  the trace already collects (`fsr::Dispatch::constants`): a 16-float window with m00, m11 > 0,
  m11/m00 within 1% of the render aspect, |m23| = 1, m33 = 0, m01 = m10 = 0. From it:
  - FOV vertical = 2·atan(1/m11); aspect = m11/m00;
  - reversed + infinite if m22 ≈ 0 and m32 > 0 (near = m32); forward + infinite if m22 ≈ -1 and
    m32 < 0 (near = -m32); finite far otherwise from m22, m32;
  - jitter (clip) = (m20, m21); in render pixels: (m20·w/2, -m21·h/2) (sign to verify against the
    title's FSR constants, which `fsr::constants` already decodes when found);
  - which buffer: the one whose form agrees with the depth (reversed), seen most often in the frame.
    Today's runs agree on 1.26578/2.25027/0.05 across 30+ pipelines.
- The value is taken from the frame being interpolated. It is latched per frame at the world upscale
  `0309a446…`, from the matrices seen since the previous present. Constants seen in frames where the
  loading composite ran are not used: loading frames get no generation. Caveat: the scan that found
  the matrix covered every traced pass in four runs, loading and world frames alike. Stage 0 must
  confirm that the 47.9°/0.05 matrix is present in world frames (frames > 2400 in run 16) and that
  m20/m21 change from frame to frame there (live jitter).
- far = infinite → pass a large far (FSR wants a number; the SDK has an infinite-depth flag, unverified).
- `viewSpaceToMetersFactor` = 1 (the title's units look like metres: near 0.05). Verify on a known object.
- Motion vector scale: FSR expects vectors in render pixels × scale, or UV space; R16G16_FLOAT at
  render size is usually UV-space or NDC-space. Determine with the stage 1 check (camera pan with a
  static scene: the vector field must match the reprojection of depth by the camera change).

Tests (pure, no GPU): the matcher on the trace's actual words for both forms (values above), rejection
of non-projection windows (the YCbCr matrix in cb `s0+48` of the video draw must not match), FOV and near
derivation, jitter extraction.

## 4. Motion vectors and depth

- Vectors: the 2560x1440 R16G16_FLOAT texture that `fsr::plan_draw` pairs with the world upscale
  (`0x1a6eba4b860` in run 16; the same object the frame's `11bbed94ee7a3243` dispatch reads). The
  `fsr::Recent` cache already remembers depth and vectors read lately by kind and size, so the hook's
  existing plan is the source. Low-res vectors flag on (render size ≠ output size).
- Depth: the 2560x1440 D32S8 object in the same plan (`0x1a5e47ef3c0`), also read by `11bbed94…`.
- Both are title textures that the title overwrites next frame. Frame generation reads frame n's and
  n-1's after the title has moved on, so we copy them at the world upscale point into our own pair
  (`fg_depth[n%2]` as R32_FLOAT via the depth/colour twin copy already in `crate::resource::TWINS`,
  `fg_mv[n%2]`), about 30 MB a frame, under 0.1 ms on this card.
- Reset flag: set on the first frame, after a swap chain resize, a frame with no composite match, a
  camera cut (projection or view jump larger than a threshold), and on toggling the switch.

## 5. Provider interface (shared with the optional DLSS frame generation)

One internal Rust trait in `crates/victory-gpu/src/fg/mod.rs`, fixed tagged inputs, nothing vendor
specific in it:

```rust
pub(crate) struct FgFrame {
    pub frame: u64,
    pub hudless: ID3D12Resource,        // 3840x2160, presented format
    pub presented: ID3D12Resource,      // same frame with the HUD (mailbox texture)
    pub depth: ID3D12Resource,          // 2560x1440 R32_FLOAT copy
    pub motion: ID3D12Resource,         // 2560x1440 R16G16_FLOAT copy
    pub render_size: [u32; 2],
    pub output_size: [u32; 2],
    pub camera: Camera,                 // fov_y, aspect, near, far (f32::INFINITY), reversed, jitter_px, view_to_clip, prev
    pub motion_scale: [f32; 2],
    pub frame_time_ms: f32,
    pub reset: bool,
}

pub(crate) trait FgProvider: Send {
    fn name(&self) -> &'static str;
    /// Records the generated frame between `prev` and `next` into `out`
    /// (presented size and format) on `list`; false: nothing generated.
    fn generate(&mut self, list: &ID3D12GraphicsCommandList, prev: &FgFrame, next: &FgFrame, out: &ID3D12Resource) -> bool;
    fn resize(&mut self, output: [u32; 2], render: [u32; 2]) -> bool;
}
```

- `FsrFg` (shipped): wraps FidelityFX frame interpolation + optical flow contexts through its DX12
  backend, driven by our list, no FFX swap chain.
- `DlssFg` (optional, never shipped): loaded only if the user points `VICTORY_DLSSG_PATH` at their own
  Streamline/DLSS-G runtime files. Behind a narrow C ABI boundary in its own module, entry points
  found by name at run time as `upscale.rs` does for NGX; no NVIDIA headers or binaries in the repo or
  in any release/test bundle. Caveat (unverified): DLSS frame generation is reachable through
  Streamline only, and Streamline wants to proxy the swap chain and needs Reflex-style markers; we would
  run it in Streamline's "manual hooking" mode with our presenter still the owner. If that mode cannot
  present through our thread, the DLSS provider is dropped, not the presenter.
- Selection: `VICTORY_FG=off|fsr|dlss|alt` (default off). `alt` toggles generation on/off every
  twenty seconds, like the other `alt` switches, for one-scene A/B in the pacing log.

## 6. FSR 3 frame generation source: licence and size

- Licence: AMD FidelityFX SDK, MIT (unverified wording; confirm `LICENSE.txt` at the pinned tag). MIT
  is compatible with our GPL-3.0 tree; keep AMD's copyright notice and the licence text with any file
  we take. FSR 4 is not open source and not used here.
- What is needed (unverified, from the 1.1 / FSR 3.1 layout): `ffx_frameinterpolation` (host ~3k lines,
  ~10 HLSL passes, ~4k lines of shader includes), `ffx_opticalflow` (host ~1.5k, ~8 passes, ~2k
  shader lines), the shared `ffx_core` headers (~6k lines, header-only HLSL/C), and the DX12 backend
  (~5-6k lines C++). Roughly 20-25k lines of C/C++/HLSL. Not needed: the upscaler, the frame
  interpolation swap chain (~3k lines, its pacing replaced by ours), samples, Vulkan backend.
- Build route: a separate crate `crates/victory-ffx-fg` with a vendored, pinned subset under
  `third_party/fidelityfx/` (MIT text alongside), compiled by `cc` (C++17) into a static lib; shaders
  precompiled to DXIL at build time with the SDK's own shader compiler step or `dxc`, embedded as
  blobs. The Rust side sees a dozen `extern "C"` functions of our own wrapper. Alternative: AMD's
  prebuilt signed `amd_fidelityfx_dx12.dll` (also MIT, unverified) loaded at run time: smallest
  integration, no C++ toolchain in our build; good for a first prototype.
- Not vendored in this step.

## 7. Staged steps, switches, verification

Each stage is off by default; each has an `alt` mode for one-scene A/B in the pacing log
(`victory_gpu::pace` lines: displayed versus real fps, present-interval p50/p95/p99, hitches,
worst second). Generated frames are counted as displayed, never as real.

| Stage | Switch | Work | Verification |
|---|---|---|---|
| 0 | `VICTORY_FSR_TRACE=1` (exists) + new `VICTORY_FG_TRACE=1` | Log per frame: each 2160p-writing candidate's hash, its written object and its loading-or-world tests (section 2); which candidate was chosen; the draws after it that write the same object (count and pipelines: the HUD); the present's object, and whether the two agree; the projection found and the FOV/near/far/jitter derived from it. | One run through a load into the world, and one menu run. Expect the loading frames to choose nothing (`ea212fca…` rejected: no vectors), and the world frames to choose `0309a446…`, with its output object matching the present. Expect HUD draws after it, and the projection stable at 47.9°/0.05 with jitter changing. Stop if the world candidate's object is not what the frame presents. |
| 1 | `VICTORY_FG_INPUTS=1` | Copy HUD-less colour, depth (R32F), vectors into our double-buffered textures at the composite; latch camera. Pure matcher and reset logic with tests. | `VICTORY_CAPTURE` of hud-less vs presented on one frame (diff shows only HUD); RenderDoc capture at the copy (markers `victory: fg inputs`); motion check: synthesise frame n from n-1 by warping with the vectors and depth in a debug compute pass (`VICTORY_FG_WARP_DEBUG=1`, shows the warped previous frame); a pan with a static scene must line up, which fixes scale and sign. Pacing log shows no cost change (inputs copy < 0.2 ms). |
| 2 | `VICTORY_FG=fsr` (+`alt`) | `FsrFg` from AMD's prebuilt DLL first (prototype), our presenter alternating generated and real frames; reset handling. | Pacing log with `VICTORY_STEADY_PRESENT=1 VICTORY_STEADY_HZ=30`: displayed 30.0, real 15.0, generated = repeats replaced (new counter `generated`), hitches 0, interval p99 within 2 ms of 33.3. Captures of a generated frame: HUD intact, no ghosting on the HUD; a pan; a camera cut (reset, no smear frame). GPU time of the generation pass from `VICTORY_GPU_TIMING`. |
| 3 | `VICTORY_FG=fsr` | Vendored source build (`victory-ffx-fg` crate), DLL route kept as fallback. | Same as 2, bit-for-bit the same generated frame as the DLL on a saved input pair (tests with recorded inputs, no game). |
| 4 | `VICTORY_FG=dlss` + `VICTORY_DLSSG_PATH` | `DlssFg` behind the C ABI, user files only. | Same pacing checks; licence check that no NVIDIA file is in `git ls-files` or release bundles (CI grep). |
| 5 | `VICTORY_FG_ADAPTIVE=1` | Generate only when real fps is below the cadence; seamless on/off with a reset. | Pacing log over a run mixing light and heavy areas: no hitch at switches. |

Measurement honesty: the pacing log keeps real fps separate (generated frames are not real frames;
input latency follows the real rate plus one real frame of hold-back, about +66 ms at 15 real fps).
The frame-rate box reads "30 FPS REAL 15".

## 8. Risks

- Below 30 real fps both vendors' interpolation degrades (research 1.1/1.2); at 15 real fps expect
  visible artefacts in fast pans. The floor lock keeps the real rate even, which helps the interpolator.
- The HUD-less point is a per-title heuristic. Two different 1440p to 2160p passes already exist
  (`ea212fca…` while loading, `0309a446…` in the world). Menus, videos and loading screens must fall
  back to no generation (reset) rather than interpolate UI. Other world modes (photo mode, cutscenes)
  may use a third pass, and stage 0 must look for them.
- Vectors' scale/sign and whether they include camera motion are not known until stage 1.
- The FSR interpolation's own pacing assumptions (it normally presents through its swap chain with
  async compute); driven from our presenter we must schedule the generation work early enough before
  the generated frame's tick (record at the real frame's publish, present at the next tick).
