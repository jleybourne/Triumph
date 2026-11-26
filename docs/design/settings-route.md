# Settings route to 30 fps, native, ray-traced lighting ON (no DLSS, no frame generation)

Design only. Nothing built, nothing run. Tags: [M] measured in a committed doc/log, [C] read in a doc or script, [E] estimate (reasoning given), [?] unknown until the named run.
Sources: research-resolution, -presets, -settings, -rt-settings, -fidelity, -60-120, -framegen; design/card-budget.md, dlss.md; scripts/run_play.ps1, run15_dlss.ps1, run-fx.ps1, run_coolfox_low.ps1.

## 0. The goal and the arithmetic

- 30 fps needs a card frame under 33 ms with the CPU phases pipelined. Card busy today ~80 ms [M, run 34]: title lists 46.0 (of which ~6-12 is ours inside its lists), our send-and-hash 28.9, compare 5.5.
- card-budget.md section 4 [E]: with the hash fix and the indirect/translator cuts the card lands at ~35 ms (range 30-42) with RT lighting OFF and the title's settings as they are. Settings must therefore remove roughly 5-10 ms of the title's own work, or the release look must stay inside that margin.
- OWNER DECISION: RT lighting is ON in the release look (`VICTORY_BVH_BUILDER=1`). That is ~+40 ms [M, one scene: CopyForBVHUpdate 13.3 + traces 24 + builder] on top of today's title work. raytracing.md track B (B1-B5) estimates traces 24 -> ~5-8 ms plus a BVH update/copy share of 3-13 ms [E, none measured]. RT-on card budget [E]: ~35 (RT off, above) + 5-8 + 3-13 = ~43-56 ms. 33 ms therefore needs ~10-23 ms more from resolution, settings trims and the RT scale. Sections 3.6 and 4 are built around that gap.
- Native route = the title renders at its own internal size and its own cheap upscale pass (<0.6 ms [M]) makes 3840x2160. We never replace that pass.
- Frame time today is CPU/copy bound, not card bound [M]. Read every result on the card columns of `framelog: usual frame` (send-and-hash, compare, "all other lists") plus `fps . ms . draws . dropped` plus a screenshot (winshot). Never judge on frame ratio (that wrongly rejected GATHER_BY_COPY).
- The settings file is already near the floor [C, research-settings]: shadows off, grass 0, ssro 2, water 256x128, tessellation off, lodScale 0.5, RT keys false. "Cut work" therefore means: (a) the resolution level, (b) a few cull/LOD/streaming knobs, (c) choosing which already-cut items to give back for a fuller picture, by measured price.

## 1. What can be switched live and what cannot

- Title settings (settings.xml) are read at launch [?: unverified; the title rewrites the file on exit]. Treat every one as a PAIRED run: control and variant, same scene, route and build.
- In-run alt exists only for OUR switches (`VICTORY_*=alt`). No title setting has one. The only in-run candidate is a window resize (research-resolution test 7): expected to do nothing; run it once (R6).
- Pairing rule: same save, same auto-advance key sequence, roam (-Roam -RoamAfter 150 -RoamMs 8000) to the same spot, measure a fixed window (the 60 s after the second W hold), report the median of 5-second `usual frame` lines with >=150 title lists. Repeat a pair if the delta is under 2 ms.
- Between runs restore settings.xml from `settings.xml.orig-backup` (not `.bak`). fsrQuality 4 must never be left in the file (black screen).

## 2. Run-script contract (write scripts straight from this)

Common header = the confirmed set in run_play.ps1, unchanged: XMEM_COMMIT 1, INPLACE 2, INPLACE_MORE 1, INPLACE_VIEW 1, ASYNC_SUBMIT 1, QUEUE2 1, INDIRECT_WINDOW 1, TEXTURE_NO_WAIT 1, MERGE_GAP 64, MERGE_RUNS 4096, STAGING_POOL 0, COPY_PLAIN 1, JOIN_PARTS 1, BACK_OWN_VIEW 1, INDIRECT_GATHER 1, CARD_REHASH_RUNS 1, CARD_WINDOWS_MB 2048, CARD_BUFFERS_MIN_KB 64, CARD_BUFFERS_MB 1280, TEXTURE_BACK 1, CARD_HASH_LEN 1, FILLS_PLAIN 1, STEP_INDEX 1, ASYNC_PRESENT 1, CARD_HASH_WIDE 1. Then:

    $env:VICTORY_FSR_TRACE = '1'   # logs upscale draws; add a log of every created target >= 1280x720 if absent
    # Native runs: NO VICTORY_DLSS, NO VICTORY_DLSS_PATH, NO VICTORY_FSR_HASHES
    # launch: run-fx.ps1 -Diag -WindowScale 0.5 -Seconds 480 -Roam -RoamAfter 150 -RoamMs 8000 [-Console series-x|series-s]
    # log:    D:\victory-work\logs\settings-<runid>.out

How profiles and settings are passed: the profile is run-fx.ps1 `-Console` (default series-x; series-s exists, the fork's run_coolfox*.ps1 use it). Settings are not passed on the command line: they are keys in the title's settings.xml. Write one helper `set-setting.ps1 -Key <name> -Value <v>`: copy `settings.xml.orig-backup` over `settings.xml`, then apply only the listed edits. Each run below lists its edits as `key=value`; no edits = control. `VICTORY_DISPLAY_MODE=WxH` (env) only changes the output size. Record per run (docs/runs.md row): internal target size, draws/frame, dropped, send-and-hash / compare / other-lists ms, median ms and fps, screenshot path, crash or black.

## 3. Measurement plan, one run per setting

"native upscale" = the title's own spatial pass to 4K. Card ms are [E] with reasoning stated; draws are [E].

### 3.1 Internal resolution (only `fsrQuality` moves it [M]; the mode list moves only the output)

Pixel ratios vs 1440p: 1260p (2240x1260) 0.77; 1080p 0.56; 900p (1600x900) 0.39. Assume 25-55 % of the title's 46 ms is pixel bound (card-budget rank 9), midpoint 40 % = ~18 ms; saving = 18 x (1 - ratio). Draw count unchanged [E]: resolution does not remove the ~4,665 draws, and the indirect commands are count bound.

| run | edit | card ms | draws | picture, native upscale |
|---|---|---|---|---|
| R0 control | none (fsrQuality 3, 1440p) | 0 | ~4,665 [M] | reference screenshot |
| R1 | fsrQuality=2 | about -4 (-2 to -7) if this level is ~1260p [?] | same | slightly soft; edges shimmer |
| R2 | fsrQuality=1 | about -8 (-4 to -12) if ~1080p [?] | same | clearly soft 4K, no temporal accumulation; UI sharp only if it follows output size [?] |
| R3 | 900p | about -11 (-6 to -15) | same | soft. FSR preset ratios have no 900p step, so a level may not reach it. Run only if R1/R2 target logs show a ~900p level. |
| R4 | fsrQuality=5, then 6 (separate runs, after R1/R2) | learn the map | | 4 gives black; stop at the first black or crash |
| R5 | VICTORY_DISPLAY_MODE=1920x1080, fsrQuality=3 | 0 to -2 [M run 6: unchanged] | same | output only; UI size check |
| R6 | resize window at runtime, no edits | expect none | | does the title re-query modes? |

The level-to-size map is unknown: read the created target sizes from the log before trusting any label. The earlier 540p "4 % saving" never changed the internal size [M], so there is no measured resolution data yet.

### 3.2 Console profile

| run | edit | expected | notes |
|---|---|---|---|
| P1 Series S style | `-Console series-s`, settings as control | If it renders: S defaults (lower internal size, smaller memory budget); card -10 to -20 ms is a pure guess [E]. Known risks: black screen, the presented 1080p target is never written (indirect group cap 65535 lead, design/ui-rendering.md), our layer gives S zero GPU-optimal memory. | Pass/fail run with screenshot and target-size log. Compare to the fork's S log. Not part of the plan unless the picture is correct. |
| P2 performance mode | no known key. Run `graphicsQualityPreset=1.0` vs 0.0 (the only candidate, a 0-1 float) and diff the settings.xml the title rewrites on exit | the diff is the result: which keys a preset moves; card ms [?] | no code; also may reveal the GI key (research-rt-settings test 7) |
| P3 quality mode | the file from P2's other extreme | the price of the title's own quality mode [?] | an upper bound for the release look |
| P4 60 fps mode | desiredFPS and frameRateLimit already 60; no separate key known | none | skip unless P2's diff shows a mode flag |

### 3.3 Settings that cut the title's GPU/CPU work (one per run, paired against R0)

| run | edit | card ms | draws | picture cost |
|---|---|---|---|---|
| S1 draw distance / LOD | lodScale=0.3 | -1 to -3 | -3 to -8 % | earlier LOD switch, mild pop-in |
| S2 small-object cull | smallCullThreshold=2.0 | -1 to -3 | -5 to -12 % | small props vanish at range |
| S3 texture mips | texLodScale_Global=2.0 | -0.5 to -2 | 0 | softer textures |
| S4 far LODs | texLodScale_SLOD1..4 doubled (8,16,32,64) | -0.5 to -1 | 0 | distant softness |
| S5 shadows | already at floor (rt* and shadow* false, directional/worldHeight 0); nothing to cut. Price of turning them on is in 3.5 | | | |
| S6 crowds / traffic | run 1: vehicleHDCount=0; run 2: vehicleLodBias=1. The population section is empty (version fields only): no crowd key is known, do not invent one; re-check after P2's diff | -0.5 to -2 near traffic | -2 to -6 % in traffic | lower-detail cars |
| S7 reflections | run 1: waterReflectionSSR=false; run 2: waterReflectionResX/Y=128/64 | -0.3 to -1.5 near water | small | blocky or missing reflections |
| S8 post | sharpenEnabled=false and fsrSharpen=0 | -0.2 to -0.6 | -2 passes | slightly softer |
| S9 AO | ssroSampleCount=0 | -0.3 to -1.5 | few | flatter contact shadows |
| S10 decoration | decorationResolutionShift=1 | -0.3 to -1 | 0 | lower-res decals |
| S11 async | asyncComputeEnabled=false | probably slower; diagnosis | 0 | none |
| S12 CPU/streaming | maxTexUpgradesPerFrame=2 | 0 card; fewer hitches and copy bytes | 0 | textures stay blurry longer |
| S13 mip bias | manualMipBias=0.5 | -0.2 to -1 | 0 | softer |
| S14 RT lighting | env `VICTORY_BVH_BUILDER=1`, no setting change | +40 now, +15 to +20 after track B [M/E] | the ~336 dropped builder draws become real | adds the missing GI (the dark look). The one run that adds picture. |

Sum of S1+S2+S3+S6+S7+S9 [E]: -4 to -12 ms. Do one combined run C1 after the singles to confirm they add up.

### 3.6 RT lighting runs (builder ON; every run here sets `$env:VICTORY_BVH_BUILDER = '1'`)

Title RT keys, named from the tree [C, research-rt-settings.md, raytracing.md B5]. Honest result: settings.xml has only TWO RT keys, both shadows, and NO RT lighting / GI / reflection key.
- `rtDynamicShadows` (advancedGraphics, bool, false): RT shadows for dynamic objects.
- `rtLODLightShadows` (advancedGraphics, bool, false): RT shadows for LOD lights.
- Related but not RT: `shadowAlphaTintedShadows`, `shadowLongShadows`, `shadowParticleShadows` (false), `directionalScreenSpaceShadowQuality` 0, `worldHeightShadowQuality` 0, `ssroSampleCount` 2, `waterReflectionSSR` true.
- RT lighting (GI) is NOT user-toggleable on this profile. RaytraceVBuffer (7.9 ms), screen probes (6.4), irradiance cache (3.5), primary probes (2.3) and RaytracingResolve [M, run light-13] run whenever the BVH builder runs. So "RT lighting on" = builder ON; no setting turns it on or sizes it.
- No RT resolution/quality key exists in settings.xml (B5's "title's own scale options" are not in the file). The binary does show a resolution ladder in the shadow trace program names: `CS_TraceShadowRays_Eighth / Quarter / Half / Full / FullAdaptive`, picked by a computed index (the reason they do not translate). The index is probably driven by a quality value, so the likeliest lever is `graphicsQualityPreset` (float 0-1) or the console profile, not a named key [?].
- Key discovery, docs/data only: (a) P2 (preset 0 vs 1) with the builder ON, diff the rewritten settings.xml; (b) search the title's archives for `Raytrac`, `probe`, `shadowRays` strings (the exe and rpf TOCs are packed, so this needs an unpack tool, not a plain grep); (c) P1/P3 profile runs: read which Trace* programs get created in the log.

RT runs (each paired against the RT control, builder ON in both):

| run | edit | card ms [E] | draws | picture |
|---|---|---|---|---|
| T0 RT control | builder ON, settings as control | about +40 vs R0 now [M]; target +10-20 after track B | about +336 real builder draws (the dropped count falls by that) | GI back, the dark look is fixed; corruption expected until the builder memory-overwrite fix (PLAN 4.4) |
| T1 | T0 plus graphicsQualityPreset 0.0 vs 1.0 | which Trace* programs are created tells the shadow step | | the first RT scale lever, if it exists |
| T2 | T0 plus rtDynamicShadows=true | 0 to +1.6; programs do not translate, expect drops | | likely inert until PLAN 4.1 |
| T3 | T0 plus rtLODLightShadows=true | same | | same |
| T4 | T0 with per-pass cardmeter (VBuffer, probes, irradiance, CopyForBVHUpdate) | gives the real split | | decides which piece to attack |
| T5 | layer trace-dispatch scaling (raytracing.md B5; switch to be written): halve trace dispatch size only where the title accumulates the output | -50 to -75 % of the trace share [E: 3-4x] | | risky; only if a title-visible setting is found or the output is proven safe |
| T6 | skip CopyForBVHUpdate/refits for unmoved objects (research-raytracing option 5) | up to -10 of 13.3 | | none if correct |
| T7 | S1-S9 and R1/R2 repeated with the builder ON | see ranking | | |

Trims re-ranked with RT on (card ms per picture loss, [E]):
1. Resolution (fsrQuality 2, then 1): VBuffer and shadow rays are per pixel, so the pixel-bound share grows; 1260p about -6, 1080p about -12.
2. T6, skip refits of static instances: -3 to -10.
3. RT scale: preset/profile if it moves the shadow ladder (shadows are only 1.6 ms of the traces); T5 trace scaling -3 to -6 of the 5-8 after track B.
4. smallCullThreshold 2.0 and lodScale 0.3: fewer instances mean fewer BVH updates and traces, so they pay MORE than with RT off, -2 to -5 each.
5. vehicleHDCount 0 / vehicleLodBias 1: fewer dynamic BVHs, -1 to -3.
6. texLodScale, water, ssro, sharpen: unchanged, small (-0.3 to -2).
7. Do not give back grass or lodScale until RT is under budget: they add refit and trace work.

### 3.4 Order and stop rules

1. R0, R1, R2 (section 6).
2. S1, S2, S3, then S6, S7, S9, S8.
3. P2 diff, then P1.
4. C1 combined, screenshot pair against R0.
5. Give-back runs (3.5).
6. RT runs (3.6) as soon as builder correctness lands (PLAN 4.4), T4 first. S14 is now the RT control T0, not a last step.
Keep a change only if the screenshot is correct and the card columns drop. "No change" is a valid result. Invalid values give a black picture or nothing.

### 3.5 Give-back runs for a fuller picture (price of restoring cut items, vs the trimmed set)

texLodScale_Mat_High=0.25 and Mat_Med=0.5 (+0.5 to +2, sharper materials); lodScale=1.0 (+1 to +4); grassLod=1.0 (+1 to +3); rtDynamicShadows, shadowAlphaTintedShadows, shadowParticleShadows=true (0 to +1.6; the shadow trace programs do not translate, so expect no change or drops); waterReflection 512x256 (+0.3 to +1.5); motionBlur=true (taste). Keep an item only if it looks right and fits the margin.

## 4. Recommended native release look (RT lighting ON)

Target: card <= 30 ms steady once our overhead is gone (wide hash, indirect cuts, translator cuts) and RT track B has landed (B1-B5 estimates, raytracing.md).

Arithmetic [E]: RT-off floor ~35 ms; RT on adds ~8-21 (traces 5-8, BVH update/copy 3-13); total ~43-56, midpoint ~50. Reaching 30 needs about -20 ms from settings and RT scale:
- fsrQuality=1 (~1080p internal, the title's own pass): about -12 (section 3.6, ranking 1).
- smallCullThreshold 2.0, lodScale 0.3, vehicle trims: -5 to -10 (fewer instances, fewer refits).
- T6 skip unmoved refits: -3 to -10.
- RT scale (preset if it works, T5): -2 to -6.
Sum -22 to -38 [E]. It closes the gap only with all of them and track B at its estimate; there is almost no slack.

Release look, in order:
1. VICTORY_BVH_BUILDER=1 (RT lighting on). rtDynamicShadows and rtLODLightShadows off (inert, untranslated programs).
2. fsrQuality=2 first; step to 1 only if the card stays above 30 ms.
3. Trim set: lodScale 0.3-0.5, smallCullThreshold 2.0, vehicleHDCount 0-2, texLodScale_Global 1.0-2.0, ssro 2, sharpen on, water 256x128.
4. No give-backs (3.5) until the card is under 28 ms.

Fallback if still over 33 ms, in order:
1. fsrQuality=1 and S1-S4 at their strongest.
2. RT scale: preset/profile step down; T5 half-res trace dispatch (only if proven safe).
3. Async compute on the host (card-budget rank 11, -3 to -8, weeks).
4. Series S profile if P1 renders correctly.
5. Otherwise report to the owner: RT lighting at native 30 fps needs the hardware-RT path (raytracing.md track A, 2-4 months [E], 10-50x on traversal) or a lower RT quality the title does not expose. DLSS is not a fallback (optional, section 5).

## 5. DLSS as an optional mode (NOT part of reaching the goal)

DLSS is still implemented in full as a user setting (Streamline path; env `VICTORY_DLSS=1`, `VICTORY_DLSS_PATH`, `VICTORY_FSR_HASHES=0309a446e1a48352,4f4ec7b16673362f`; run15_dlss.ps1 is the template). It replaces the title's upscale pass only. The release look above stands with it off and is judged without it.
- Internal resolution with it on: 1080p (the R2 edit) as the default, Quality-to-Balanced class: about -8 ms vs 1440p [E] and a temporal picture instead of the soft spatial one. 1260p is the quality option (about -4 ms). 900p (Performance class) stays opt-in and only once motion vectors and jitter are verified (dlss.md section 2).
- DLSS itself costs about +1 ms at 4K [E]. Judge each resolution by a same-spot screenshot pair in motion against R0.
- Optional runs after the native plan, reported separately: D1 (1080p), D2 (1260p), D3 (900p) = the R2/R1/R3 edits plus the three DLSS env vars.

## 6. First three runs

1. R0: control, no edits, FSR_TRACE on; record target size and card columns.
2. R1: fsrQuality=2 only; read the target size (about 1260p?) and the card delta.
3. R2: fsrQuality=1 only; read the size (about 1080p?), card delta, screenshot pair at the same spot.
Then T0 (builder ON control) and T4 (per-pass split) once the builder fix lands. These give the title's pixel-bound share, which fixes the whole recommendation (card-budget section 4 depends on it).
