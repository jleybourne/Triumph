# UI rendering under the layer (design, nothing built or run)

## 1. Diagnosis
VERIFIED
- queue.rs `present_now` (and `send_behind`, the async path) reads only `plane_parameters.as_ref()`, i.e. plane 0, resource 0. Planes 1..n are never looked at: dropped. The `Plane` struct (queue.rs ~1119) is also 0x18 bytes where the title's stride is 0x48 (upstream asserts 0x48), so even indexing planes[1] is wrong in our tree.
- host.rs `Host::present_now` copies one texture 1:1 (`CopyResource`) into a swap chain sized to it (3840x2160, fmt 28 in every log). No scaling, blending or colour conversion.
- Upstream 0736f8b3 "composite multiple PresentX planes (Series S UI)" is on mv/main only, not an ancestor of pipeline-pump. It sizes the swap chain to the largest plane, draws every plane as a full-screen triangle, plane 0 opaque, later planes with premultiplied blend (ONE, INV_SRC_ALPHA), linear sampler, max 8 planes, falls back to plane 0 if the compositor fails. No per-plane rects, no HDR, no resolution handling.
- No log line records the plane count or plane sizes, so whether the title submits >1 plane is unmeasured.
- captures: targets.png shows a control-hint box (HUD) in plane 0; fsr3-playing and fsr4-fixed show clean scene and no HUD; series_s*.png are pure black.
- settings.xml: uiOverlay false, uiResolution 0x0 (auto), hdr false (SDR path).

INFERRED
- Menus/HUD missing in most captures is the dropped plane(s) 1+, when the title uses a separate UI plane. The hint box appearing means some UI is baked into plane 0 in that state.
- Series S black is a different bug: docs/remaining-shaders.md shows the presented 1080p target is never written (indirect group cap 65535 lead). Compositing will not fix it.
- Red block artefacts in fsr4 are unrelated (shader/memory).

## 2. Design
- Order: plane 0 = game (opaque, overwrite), planes 1..n in submission order over it. Treat console order as back to front.
- Alpha: premultiplied (ONE, INV_SRC_ALPHA, alpha same), matching the console compositor and upstream. Straight-alpha UI would show dark fringes; verify with one RenderDoc pixel.
- Resolution: UI plane is usually a fixed 1920x1080 (or 4K) independent of the game plane (dynamic with FSR). Swap chain = game plane size (not max), UI stretched to it; honour per-plane source/dest rects from the parameters (`_rest` 6 u64s, layout unverified; decode from a capture first).
- Filter: bilinear for UI upscale; point if integer ratio; game plane 1:1 stays CopyResource fast path.
- HDR/SDR: hdr=false now: the UI plane is already sRGB, blend in the swap format. When HDR on: UI plane scRGB/PQ-converted with UI white nits (hdrPeakBrightnessUIDefaultNits 600), game plane separately; defer.
- Cost: one 3-plane pass at 4K is ~0.3 ms GPU plus one extra barrier set, versus 2 ms present now; single-plane frames pay nothing.

## 3. Staged plan (switch VICTORY_UI_PLANES=0|1|log, default 0)
0. Log only: fix `Plane` to 0x48 in queue.rs; in `present_now` log plane count, per-plane resource size/format and `_rest` words once per change. Zero behaviour change. Run once.
1. Port 0736f8b3 Compositor into host.rs (`Compositor`, `Host::present(&[(tex,state)])`), `present_now` and `send_behind` pass all planes; Held::Present carries a Vec. Keep the plane-0 fallback.
2. Sized swap chain to plane 0; scale smaller/larger planes by rect (new constant buffer root param).
3. Rects, alias-state handling (upstream test aliased_present_planes...), skip when VICTORY_ASYNC_PRESENT unsupported.
4. HDR/UI-nits path (only if hdr true).

## 4. Verification
- Stage 0: grep log for plane lines; if count is always 1, UI is not a plane issue; stop.
- Stage 1: VICTORY_CAPTURE=<dir> VICTORY_CAPTURE_FRAMES=<n> on a menu frame, with switch 0 vs 1; diff PNGs, UI pixels only.
- RenderDoc: VICTORY_CAPTURE_TOOL=renderdoc VICTORY_CAPTURE_AT=<n> VICTORY_MARKERS=1; confirm composite events, plane textures, blend state, one pixel premult check.
- Tests: upstream compositor unit tests, plus a synthetic 2-plane test with half alpha.
