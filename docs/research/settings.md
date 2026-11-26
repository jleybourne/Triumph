# Settings research (read-only; nothing run, nothing edited)

Sources: settings.xml (current values) and docs/settings.md on main. Ranges below are PLAUSIBLE, inferred from
names and typical engine conventions; none were verified in this title. Treat every range as a guess until a run shows it.

## Observation: the file is already near the floor
Most quality knobs already sit at minimum (grassLod 0, shadows off, ssro 2, water 256x128, tessellation off, motion blur off,
MSAA off, aniso 0, HDR off). Remaining headroom is smaller than the list suggests; the render scale (fsrQuality) dominates.
Render scale check: 2560/3840 = 0.667, which matches an FSR "Quality" ratio, consistent with fsrQuality 3 = 1440p scene to 4K output.
Note the file also says resolutionFullscreen 854x480 and videoCardDescription "Xbox Series X": these are reported values, and the doc says render targets come from fsrQuality, not from them.

## All settings: current value, plausible range

graphics
- hdr false (bool); hdrIntensity 50 (0-100); hdrPeakBrightnessUIDefaultNits 600; hdrPeakBrightnessGameDefaultNits 1000; hdrPeakBrightnessManuallyChanged false; hdrPeakBrightness 1000 (nits); hdrBlackLevels 50 (0-100); hdrFilmicMode true. All HDR items: output only, no GPU effect while hdr=false.
- gamma 15 (about 0-30); anisotropicFiltering 0 (0,2,4,8,16); desiredFPS 60 (30/60/uncapped?); fsrQuality 3 (0-4; 2 old baseline, 3 best, 4 no gain on newest build); graphicsQualityPreset 0.0 (0-1, probably a master slider; may overwrite others, leave alone).

advancedGraphics
- asyncComputeEnabled true (bool); damageModelsEnabled true (bool); decorationResolutionShift 0 (0-3, shift = halve per step); directionalScreenSpaceShadowQuality 0.0 (0-1); fsrSharpen 0.2 (0-1); grassLod 0.0 (0-1); lodScale 0.5 (about 0.2-1.0+; lower = nearer LOD switch).
- texLodScale_Global 1.0 and _Mat_High/_Med/_Low, _Buildings, _Objects, _HD, _OrphanHD all 1.0 (0.5-4; higher = lower-res mips); _LOD 2.0; _SLOD1 4.0; _SLOD2 8.0; _SLOD3 16.0; _SLOD4 32.0 (the doc says 1..32).
- smallCullThreshold 1.0 (0-? higher culls more small objects); maxTexUpgradesPerFrame 5 (1-16, streaming); lensFlaresScreenResDivExponent 2 (0-4); motionBlur false; motionBlurLimit 16.0; motionBlurScale 1.0.
- rtLODLightShadows, rtDynamicShadows, shadowAlphaTintedShadows, shadowLongShadows, shadowParticleShadows: all false (bool, already off).
- sharpenEnabled true; sharpenIntensity 1.0 (0-2); ssroSampleCount 2 (0-16); transferQueuesEnabled true; treeTessellationEnabled false.
- vehicleLodBias 0.0 (about -1..+2?); vehicleHDCount 2 (0-8); volumetricsRaymarchResolutionUnclamped false (bool; true raises cost); waterReflectionResX/Y 256/128 (power of two, 64-1024); waterReflectionSSR true (bool).
- worldHeightShadowQuality 0.0 (0-1); mapMSAA false; vrsEnabled false (inert, proven); manualMipBias 0.0 (about -2..+2; positive = blurrier, cheaper).

video
- uiOverlay false; uiResolution 0x0 (auto); adapterIndex 0; outputIndex 0; refreshRate 60/1; resolutionFullscreen 854x480; tripleBuffered false; frameRateLimit 60; vSync 0 (0/1/2).

misc: audio.lowFrequencySoundUpdate false (CPU, irrelevant); ai, population, physics sections empty (only version fields).

## Ranking: expected GPU-time cut per visual loss (best first)
1. fsrQuality: biggest lever but 3 is already in and 4 shows no gain; do not touch except to revisit 4 later (black screen risk).
2. lodScale 0.5 is already low; trying 0.3 cuts geometry cost, mild pop-in.
3. texLodScale_Global 1.0 -> 2.0: lower mip bandwidth/memory; cheap visually at 1440p-to-4K.
4. smallCullThreshold 1.0 -> 2.0: culls small objects; helps draw count and GPU; some pop.
5. sharpenEnabled true -> false (plus fsrSharpen 0.2 -> 0): fullscreen pass at 4K, real but small ms; mild softness.
6. asyncComputeEnabled true -> false: may help or hurt; async compute usually saves time, so test is mainly for diagnosis.
7. vehicleHDCount 2 -> 0 and vehicleLodBias 0 -> +1: small saving, only matters with traffic.
8. decorationResolutionShift 0 -> 1: halves decoration res; small.
9. waterReflectionSSR true -> false, or waterReflectionRes 256x128 -> 128x64: already low; matters only near water.
10. ssroSampleCount 2 -> 0 (or 1): probably small as it is already 2.
11. directionalScreenSpaceShadowQuality / worldHeightShadowQuality: already 0, nothing left.
12. manualMipBias 0 -> +0.5: small; softens.
13. maxTexUpgradesPerFrame 5 -> 2: streaming/CPU/memory, not GPU time; relevant to the CPU memory fix, may reduce hitches.
14. lensFlaresScreenResDivExponent 2 -> 3, damageModelsEnabled -> false: negligible.
Not worth testing: grassLod, tessellation, motion blur, MSAA, shadow toggles (all already off/0), vrsEnabled (inert), HDR items.
Avoid raising: volumetricsRaymarchResolutionUnclamped true, anisotropicFiltering.

## Frame caps (flag)
- desiredFPS 60, frameRateLimit 60, vSync 0, tripleBuffered false, refreshRate 60/1.
- A cap of 60 cannot block 30, 45 or even 59 fps. vSync 0 means no vsync-quantised halving. Nothing in the file says 30.
- The menus at exactly 30.0 fps therefore are not explained by this file: probably a hard-coded menu cap in the title or the layer. Do not assume it also caps gameplay.
- At 45 ms/frame (about 22 fps) no cap binds at all. A 30 cap only becomes relevant once frame time drops under 33 ms.
- Only test caps after GPU time improves: frameRateLimit 60 -> 0 or 120, then desiredFPS 60 -> 120, to see whether a hidden 30 exists. Leave vSync 0 and tripleBuffered false.

## Test order (one setting per run, screenshot each time)
1. Control: no change, confirm fsrQuality 3 baseline ms and a screenshot.
2. texLodScale_Global 1.0 -> 2.0. Watch for: blurry textures (expected), black screen, no change (setting may be ignored).
3. lodScale 0.5 -> 0.3. Watch for: pop-in, crash, no change.
4. smallCullThreshold 1.0 -> 2.0. Watch for: missing small props, draw count drop.
5. sharpenEnabled true -> false. Watch for: softness; no change in ms means pass is cheap.
6. asyncComputeEnabled true -> false. Watch for: slower (likely), flicker, crash.
7. decorationResolutionShift 0 -> 1. Watch for: no change, odd decoration.
8. vehicleHDCount 2 -> 0 (separate run: vehicleLodBias 0 -> 1). Watch for: no change away from traffic.
9. waterReflectionSSR true -> false (separate run: 256x128 -> 128x64). Watch for: black or missing water.
10. ssroSampleCount 2 -> 0. Watch for: dark artifacts, no change.
11. manualMipBias 0 -> 0.5. Watch for: blur, no change.
12. maxTexUpgradesPerFrame 5 -> 2 (CPU/stream side). Watch for: lingering low-res textures, fewer hitches.
13. Caps last, after GPU time is under 33 ms: frameRateLimit 60 -> 0, then desiredFPS.
Keep a change only if the screenshot is correct and the median ms drops; otherwise restore from the backup.

## Rules (from docs/settings.md)
- One setting per run (both past failures were attributable only because of this).
- Screenshot (winshot) every run; a frame counter can look fine over a black picture; the log is not evidence.
- Keep settings.xml.orig-backup untouched; the title rewrites settings.xml on exit, so a bad value persists to the next run. Restore from the backup (not .bak, which is just the previous copy) after any failure.
- Invalid values give a black picture or silently do nothing; "no change" is a valid outcome to record.
- fsrQuality 4 must not be left in the file.
