# The title's settings

The title keeps its graphics settings in a plain XML file in its own storage, and it **reads them at
launch**:

```
<data>/storage/settings/settings.xml          (settings.xml.bak is the previous copy)
```

This is the only surface found so far that changes **what the title renders** rather than how the layer
services it. Everything below was measured on the machine the README names, with a backup kept at
`settings.xml.orig-backup`.

## What is in it

```
graphics            hdr · gamma · anisotropicFiltering · desiredFPS · fsrQuality
                    graphicsQualityPreset
advancedGraphics    asyncComputeEnabled · lodScale · grassLod · texLodScale_* (1..32)
                    maxTexUpgradesPerFrame · ssroSampleCount · waterReflectionSSR
                    waterReflectionResX/Y · shadow*/rt* toggles · vrsEnabled · mapMSAA
                    vehicleLodBias · vehicleHDCount · volumetricsRaymarchResolutionUnclamped
video               resolutionFullscreen (854x480) · refreshRate · frameRateLimit · vSync
                    uiResolution · adapterIndex/outputIndex
```

`fsrQuality` is the internal render scale: FSR's quality level *is* the render-scale setting, which is
why reporting a smaller display mode to the title (`VICTORY_DISPLAY_MODE`, down to 426x240) never
changed its render targets — the title derives them from this number, not from what the console
reports.

## Measured

| change | result |
|---|---|
| `fsrQuality` 2 → 4 | **black screen.** Reverted. The title's FSR path does not produce a picture at that level here. |
| `vrsEnabled` false → true | game runs normally (3,908 draws, 135.3 ms) but **`0` shading-rate calls in the log**: the title never asks for a variable rate on this profile, so the setting is inert. |
| `fsrQuality` 2 (original) | 7.6 fps median · 131.3 ms at ~4,500 draws — the baseline in `runs.md` |

## Rules for changing it

1. **One setting per run.** Both failures above came from a single edit, and both were only
   attributable because nothing else changed.
2. **Screenshot before believing.** A frame line can read 7 fps while the picture is black; the
   `winshot` capture is the check, not the frame counter.
3. **Keep `settings.xml.orig-backup`.** The title rewrites the file on exit, so a bad value can be
   persisted by the next run.
4. **Invalid values are not rejected loudly.** They produce a black picture or a setting that silently
   does nothing, so the log is not evidence — the capture is.
