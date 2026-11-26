# Reaching a stable 30+ displayed fps: frame generation, reprojection, pacing, cheaper content

Status: desk research from prior knowledge. No web pages were fetched in this pass, no builds, no game runs.
Numbers marked (unverified) are from memory and must be checked against vendor docs before they drive a decision.
Project and product names are deliberately omitted here; see private/framegen-sources.md.

## 0. Framing and the plain-speaking part

- Today: ~7-8 real fps (125-140 ms/frame), CPU-bound in our layer. Expected in weeks: 15-20 real fps. The GPU (4070 Ti) is mostly idle at those rates, which is ideal for frame generation (spare GPU time) but says the bottleneck is ours, not the hardware.
- Goal arithmetic: 2x frame generation turns 15-20 real into 30-40 displayed. With 2x, a real floor of 15 fps is the exact edge of 30. Any real dip below 15 shows as a displayed dip below 30. "No dips" therefore needs a real floor of roughly 16-17+ with 2x, or a smoothing layer that holds the displayed rate steady through real-frame stalls (sections 2 and 3).
- Do generated frames count toward the owner's goal? Honestly: they count as displayed frames and the picture will look like 30+ fps. They are not new game simulation and not new input sampling. Input-to-photon latency stays tied to the real frame rate and gets worse (see 1.4). If the owner's definition is "feels like 30 fps to play", generated frames alone do not meet it. If the definition is "looks smooth at 30+ on screen", they do. This should be stated in any report so nobody is misled by an overlay counter.
- The cheapest big win is not any of the bridging techniques: 7-8 fps CPU-bound suggests pathological per-frame costs (synchronous waits, readbacks, descriptor churn, pipeline compiles, per-draw heavy work). Frame generation on top of a 7-8 base is poor; on a 15-20 base it is marginal-to-acceptable; on 30 base it is good. Keep optimizing the layer; bridging is a multiplier, not a substitute.

## 1. Frame generation

### 1.1 Vendor ML frame generation with a user-supplied signed DLL (via the vendor's integration SDK)
- Mechanism: game supplies per-frame depth, motion vectors, HUD-less colour, optional UI colour+alpha, camera matrices/parameters, jitter, and the SDK interpolates one frame between two real frames using a hardware optical-flow unit (older model) or a newer ML model that no longer needs the hardware unit on 40-series (unverified which models the user DLL offers). The SDK wraps the swapchain, owns presentation of generated frames, and expects a latency-marker API (Reflex-style PC latency markers) to be called around simulation/render/present.
- Inputs we have: 2560x1440 RG16F motion vectors, depth. Still needed: (a) a HUD-less colour copy, which in a translation layer means detecting "last 3D pass before UI" with a per-title heuristic; (b) UI colour+alpha target, or accept that UI gets interpolated and ghosts; (c) camera data: view/projection or camera-to-clip, previous-frame matrices, near/far/FOV, jitter offsets, motion vector scale and sign convention, reversed-Z flag. Because we are the translation layer, these must be sniffed from constant buffers or inferred. This is the real engineering cost.
- Base-rate minimums: vendors and reviewers usually quote a comfortable base of 40-60 fps; artifacts (smearing around fast objects, UI warping, disocclusion halos) grow fast below ~30 base (unverified specifics). At 15-20 base, expect obvious artifacts in motion, especially on camera pans.
- Latency: the interpolator must hold back the newest real frame until the in-between frame is shown, so added latency is about one real frame time plus the FG cost: roughly +50 ms at 20 base, +66 ms at 15 base, on top of native pipeline latency. A 15 fps title is already 70-130 ms; end-to-end 150-250 ms is plausible. A latency-marker integration helps only when a render queue exists; a CPU-bound layer has little queue to trim.
- GPU cost: on the order of 1-2 ms at 1440p on this class of card plus a few hundred MB of VRAM (unverified). Irrelevant to a CPU-bound title.
- Platform requirements: hardware-accelerated GPU scheduling on, flip-model swapchain, swapchain created through the SDK proxy, Reflex-style markers. 2x only on this card (multi-frame generation is current-gen only).
- Effort: high. Swapchain proxying, resource tagging, camera-data extraction, HUD-less capture, latency markers, load-on-demand plugin boundary. Realistically 3-6 weeks to get artifact-tolerable output on one title.
- Licensing: the integration SDK source is permissively licensed (MIT-style) and GPL-compatible to link; the signed ML DLL is NVIDIA proprietary under its own SDK licence. We never ship it; the user supplies it and we load it dynamically behind a narrow C interface with no proprietary headers copied into the repo. This is the usual practice; confirm with counsel because GPL "system library" arguments are not settled. Do not distribute the DLL in releases or test bundles.
- Effect on "no dips": FG paces its own presents well, but it needs steady real frames. Irregular real arrival (12 fps then 20 then 14) produces uneven interpolation intervals and latency swings; FG hides some of this but cannot cure a real frame that takes 120 ms. Partial help.
- Caveats: black box, hard to debug; version drift between SDK and DLL; vendor-locked to one GPU brand; first-frame/reset handling after loading screens must be correct or you get one-frame corruption.

### 1.2 Open-source frame interpolation (shippable, any GPU)
- Mechanism: same family (interpolate between two real frames) but implemented as compute shaders in an MIT-licensed SDK. It runs its own optical-flow pass in addition to the game's motion vectors, composes the interpolated frame, and ships a replacement swapchain/pacing layer that presents real and generated frames on a timer with async compute. Newer revisions decoupled it from the upscaler so it can be used alone.
- Inputs: depth, motion vectors, HUD-less colour (optional UI composition callback), camera near/far/FOV, frame delta, reset flag. Notably lighter on camera data than 1.1 (no full matrices), which is a large saving for a translation layer.
- Base-rate: vendor guidance is a minimum around 30 real fps with 60 recommended (unverified); below 30 quality and pacing degrade sharply, same as 1.1.
- Latency: same one-frame hold-back; same arithmetic as 1.1. It has historically been prone to pacing stutter when GPU-bound; here GPU has headroom, which helps.
- Effort: medium-high. Easier than 1.1 on camera data, still needs HUD-less capture and swapchain replacement. Source is available, so debugging is possible. 2-4 weeks.
- Licensing: MIT, compatible with GPL-3.0; can be built into our tree and shipped. No user-supplied binary.
- Effect on dips: same as 1.1; its pacing component may be directly reusable as the presenter for section 3.
- Caveats: visual quality is generally a notch below the vendor ML path; it is more sensitive to bad motion vectors; HDR and UI handling need care; its swapchain replacement will fight any presenter we already own, so choose one owner of presentation.
- Recommendation: build both behind one internal "frame-generation provider" interface that takes a fixed tagged-input struct (depth, MV, HUD-less colour, UI, camera). Do the shared input extraction once; the providers are then small. Prototype the open one first (no legal friction, testable by anyone), then add the plugin one for quality.

### 1.3 Post-process frame generation without engine inputs
- Mechanism: take the final presented image only, estimate motion from consecutive finished frames, interpolate. Some are GPU-vendor driver features (limited vendor/generation support, unverified for this card) and some are third-party overlay tools that work on any GPU and can capture any window.
- Numbers: they handle 2x-4x and beyond, but have no depth or MVs, so UI, particles and thin geometry smear; quality at 15-20 base is poor.
- Effort: near zero for a stopgap (the user runs the tool on our window); moderate to embed.
- Licensing: external tools are separate programs, no impact on us. Embedding their code is not possible (proprietary). Writing our own is a research project.
- Effect on dips: little; same caveats as 1.1, plus worse artifacts.
- Value: a zero-code A/B reference and a fallback for the owner's own demos. Not a deliverable of ours.

### 1.4 What other translation layers and emulators have done
- I could not verify specific emulator FG integrations in this pass. The common pattern I know of: emulators and layers typically offer spatial upscalers and presentation tweaks; true frame generation has mostly arrived as (a) user-level injection mods that hook the swapchain and replace one vendor's FG DLL calls with another's implementation, (b) title-specific patches, or (c) external post-process tools. Mainline integration is rare because of the camera-data and HUD-less requirements, which a generic layer does not know. Treat claims otherwise as unverified until sources are checked.
- Lesson for us: the cost is the per-title input extraction, which we can pay because this is one title.

## 2. Asynchronous reprojection and extrapolation

### 2.1 Rotation-only late reprojection (VR-style "timewarp")
- Mechanism: after the real frame is rendered, re-present it warped by the camera rotation that happened since the pose used to render it (a homography, no depth needed). For a flat game the "pose" is the game's camera, which we only learn by sniffing view matrices; the late pose comes from the latest stick/mouse input applied to the last known camera, or from extrapolating angular velocity.
- Evidence: standard in VR for 20+ years; vendor "frame warp" features for esports now do input-late warping with inpainting for the exposed edges. Rotation-only warp is robust and cheap (sub-ms).
- Value here: at 15-20 real fps it makes camera look feel responsive and fills in intermediate displayed frames for camera motion. It does not move characters or objects, so character animation still shows 15-20 fps judder.
- Effort: medium. Needs view-matrix extraction (same as 1.1), a late-latch input path, a presenter thread (section 3), and edge filling. 2-3 weeks for a first version. No licensing issues; we write it.
- Dips: strongest "dip hider" for camera motion because it is decoupled from real frame time.
- Caveats: edge disocclusion at borders (crop or mirror/stretch fill), UI must be composited after the warp, and the camera must be inferable. If the game applies its own camera smoothing, extrapolation can overshoot.

### 2.2 Positional reprojection and motion-vector extrapolation
- Mechanism: use depth to correct translation, and MVs to push dynamic objects forward (forward warp with hole filling), producing a predicted frame without waiting for the next real one. Avoids FG's one-frame hold-back.
- Evidence: shipped in VR as spacewarp-class features at 45->90 fps; research literature on frame extrapolation exists with ML inpainting. Quality at 15-20 base for a third-person/open-world camera is unproven; the prediction horizon (50-66 ms) is far longer than the VR cases it was designed for.
- Effort: high and research-grade. Quality risk is high.
- Dips: helps latency, may help dips, but artifacts rise with the extrapolation horizon.
- Recommendation: do 2.1 first; consider 2.2 only if latency complaints dominate.

## 3. Decoupled presentation and frame pacing

- Mechanism: a dedicated present thread owns the swapchain and runs at a steady cadence. The game/translation thread publishes completed frames into a small mailbox. At each present tick the presenter shows the newest completed frame (or its reprojected/interpolated variant), re-presenting the last one if none arrived. Uses a flip-model swapchain with a frame-latency waitable object and tearing allowed with variable refresh.
- Numbers: with a variable-refresh display, uneven real frames at 15-20 fps look better presented as they arrive than force-fed to a fixed 60 Hz; low-framerate compensation on the display multiplies repeats below its range. Without variable refresh, steady 30 Hz presenting of a stable 15 real fps (each shown twice) is smoother than 12-20 irregular fps on a 60 Hz screen.
- Value: eliminates presentation-side stutter, makes the overlay counter steady, and is the prerequisite for sections 1 and 2 (they need one owner of present).
- Frame-limiting: capping real production at a rate slightly below the sustainable minimum (for example locking to 15 when 15-20 is achievable) trades peak for stability. Dips are felt more than averages; a lock at the floor is the cheapest "no dips" tool.
- Effort: low-medium (days to a week). No licensing issues.
- Honest caveat: duplicated frames inflate "displayed fps" without adding new picture. State it plainly in any measurement. Show real fps and displayed fps as two separate numbers in our overlay.

## 4. Making the title itself cheaper

I do not know this title's exact settings list; below is ranked by likely effect on CPU-side command count in a translation layer, from general engine behaviour. Measure, do not trust.

Principle: CPU cost in a layer scales with draw/dispatch count, state/descriptor/barrier changes, and resource uploads. Resolution scale, dynamic resolution and most texture/post-process quality cut pixels, not commands, and will do almost nothing for a CPU-bound run.

Likely highest to lowest:
1. Shadow quality/cascade count/shadow distance: every cascade re-submits scene geometry. Reducing cascades or shadow-casting distance can cut a large fraction of total draws. Often the single biggest lever.
2. Draw/view distance, object LOD bias, and instance culling distance. Directly cuts draws.
3. Crowd/NPC/traffic density and animation quality. Skinned draws plus animation uploads; strong lever in open worlds.
4. Foliage/vegetation/clutter density, particle counts, decals.
5. Reflection method and update rate (cubemap/planar updates re-render the scene).
6. Console profile: a "performance" or 60 fps profile is designed to fit a ~16 ms game-thread budget, so it commonly cuts CPU-side work (draw distance, crowds, simulation detail) beyond resolution; the "quality" profile does the opposite. The smaller-console profile typically also lowers LODs and shadow/crowd budgets. Expect the performance profile on the smaller-console settings to be the cheapest, but verify whether the 60 fps mode drops simulation tick or changes fixed-step logic, which could interact with our low real fps (catch-up stepping, slow motion).
7. Post-process/AO/resolution presets: GPU-side, near-zero CPU effect. Leave at whatever looks best since the GPU is idle.

Protocol (no game runs by me): add per-frame counters in the layer (draws, dispatches, barriers, descriptor writes, bytes uploaded, pipeline binds). Run one fixed camera path per setting change, one setting at a time, record counts and ms/frame. Rank by command-count-per-quality-lost. Check also for config-file or console-variable overrides the engine exposes for the owner's own copy; these often go beyond menu limits.

Expected effect on "no dips": large; fewer commands raises the floor in heavy areas (dense crowds, large vistas), and heavy areas are exactly where dips occur.
Effort: low. Licensing: none. Caveat: owner may not accept visibly reduced content for a performance claim; document the profile used.

## 5. Other ideas

- Pipeline and shader cache with pre-warm: most "dips" in layers are first-use pipeline compilation and shader translation, not steady cost. Persist a cache and pre-compile on load, off the render thread; show a one-time warm-up. High impact on dips, low-medium effort, no licensing issue.
- Parallelise command translation: record command lists on worker threads, avoid per-draw locks, batch descriptor writes, bind/pack tables, use indirect execution to collapse draw runs. This is the main route from 7-8 to 15-20+. Amdahl: only the translation hot loop matters; profile first (CPU sampling by function).
- Command-list replay/cache: if the game re-submits identical sequences (static geometry passes), cache the translated lists and replay with patched constants. High payoff, high complexity, correctness risk; only after profiling shows a stable repeated sequence.
- Temporal amortisation of passes (e.g. update the static shadow cascades or reflection probes every Nth frame): large CPU savings but changes the title's behaviour and needs pass identification; treat as a later, optional, flagged feature.
- Thread/core hygiene: pin the hot translation thread to a physical core with its hyper-thread sibling idle, high-performance power plan, large-page allocations, avoid cross-thread cache bouncing. Single-digit percent gains but nearly free.
- Hardware reality: this CPU is a 2020 8-core. A recent CPU with much higher single-thread speed would be a straight 1.5-2x on a single-thread-bound layer. Out of scope for software, but it is the most reliable lever; say so to the owner once.
- Adaptive FG: enable generation only when real fps is under a threshold, so good areas stay native and low-latency. Needs seamless toggling (no flicker).
- Honest labelling: overlay shows "real N / displayed M"; do not report M as the achievement.

## 6. Ranked expected win toward "stable 30+ displayed, no dips"

1. Cut the layer's CPU cost and kill first-use stalls (profiling, parallel translation, persistent pipeline cache): the only route that raises real fps and removes dips. Highest win per effort.
2. Cheaper title settings/profile chosen by measured command counts: low effort, raises the floor in heavy scenes.
3. Single-owner decoupled presenter with frame pacing and a real-fps floor lock: low effort, directly steadies the displayed rate.
4. Open-source frame interpolation provider behind a shared input-extraction layer: shippable, brings 15-20 real to 30-40 displayed; latency +1 real frame; artifacts at low base.
5. Plugin vendor-ML frame generation (user-supplied DLL, Streamline-style): best quality, highest integration cost, the inputs are shared with 4.
6. Rotation-only late reprojection: moderate effort, strong at hiding camera-motion judder and dips, partial.
7. Motion-vector extrapolation and external post-process tools: speculative or stopgap.

Order of work: 1 and 2 continuously; 3 next (days); then shared input extraction (HUD-less capture, camera data) enabling 4/5/6.
