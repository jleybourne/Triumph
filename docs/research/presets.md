# Preset research (read-only)
VERIFIED
- <game dir> top level: no loose settings/preset/graphics/quality/profile files. Only appxmanifest.xml, layout_*.xml (package layout), MicrosoftGame.Config, README.txt. data/ holds only ttdb/, pack/ only dirs.
- Targeted grep of the main exe (no full scan): none of graphicsQualityPreset, fsrQuality, lodScale, texLodScale, QualityPreset appear in plaintext (code/strings packed or in archives). Only hit "profileSettingsPreset" = unrelated Xbox profile string.
- .rpf: magic "9FPR" (header readable, TOC/entries not checked); small ones (appdata0 8.9KB, common 6.6MB, script 4.3MB, data 480MB) not decompressible here. No readable preset definitions found.
- Layer: Series S differs only in (a) device-type 0x60 vs X, (b) memory budget: X = 10 GiB optimal + 3.5 GiB standard, S = 0 optimal + 7776 MiB standard, (c) name string. Source comment: title "picks settings by it" (resolution, memory).
- settings.xml: a single graphicsQualityPreset float, no preset table, no comments.
INFERENCE
- Presets table cannot be produced from files. Likely the title ships per-platform defaults in archives (RAGE fwSystemSettings), selected by device type; Series S black screen probably from the zero GPU-optimal budget or an S render path unsupported by the layer, not a settings value.
- graphicsQualityPreset is a float, so probably a continuous slider (0-1) used by the in-game menu to write the per-setting values at change time. xml stores every derived value individually, so launch most likely loads stored values. Unverified; a test decides.
- 0.0 with everything already at floor suggests 0.0 = lowest.
