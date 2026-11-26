# Fidelity gap research (read-only)

## Verified
- Latest run frame line: "4665 draws/frame, dropped 339 without a host pipeline". Per-line breakdown: 336 = ray-tracing builder stage (builder off), only ~4 are real untranslated or refused shaders (3 refused compute, 1 compute). So the "345 dropped" is about 97 percent the BVH switch, not shader gaps. 4 distinct untranslated hashes in the 11-26 evening logs.
- settings.xml vs .bak (old): desiredFPS 30->60, fsrQuality 2->3, grassLod 1->0, lodScale 1->0.5, texLodScale_Mat_High 0.25->1, Mat_Med 0.5->1, motionBlur true->false, rtDynamicShadows true->false, shadowAlphaTinted true->false, shadowParticle true->false, waterReflection 512x256->256x128, resolutionFullscreen 3840x2160->854x480, frameRateLimit 30->60, vSync 2->0. orig-backup vs settings.xml: ONLY fsrQuality 2 vs 3 (ignoring CRLF; plain diff shows every line). Note Mat_High/Med 0.25/0.5 in .bak mean sharper material mips, a higher quality than 1.0 here.
- Captures: fsr4-fixed-211141 shows a near-black room, orange rim light only, no bounce or ambient fill, crushed shadows, and red blocky corruption on the jacket (4x4-aligned smears, texture or shader). fsr3-playing shows the same: hair/window lit, rest black. Consistent with missing GI/probes.
- Logs: "card has no room for new ones so_far=45000" appears 167 times (texture slot budget exhausted: new textures not created). "buffer views ... refused and read zeros" is now mostly fixed (205 kept). No "not supported" lines.
- design/gpu-textures.md: 12,030 waiting, 8,128 unmapped (a mapping state, not a cost); XOR tile modes (render-target class) are not CPU-detiled.

## Inferred
- Dark look: VBuffer/probes/irradiance cache (about 20 ms of the 24 ms trace cost) are the GI. With them dropped the title's lighting has no indirect term. Largest single gap.
- Release look also loses shadows (CS_TraceShadowRays_* untranslated, never created by the title, so rtDynamicShadows may be inert even on).
- Red blocks: BC decode (BC2 undecoded, BC5/BC7 modes) or unmapped partial mip upload; undecided until PLAN 1.5 RenderDoc.
- Whether the slot-budget "no room" message correlates with blur is untested.
- settings.xml values are probably honored at launch, but lodScale/texLod effects are unverified in this title.

## Ranked: visual impact per ms
| # | Missing | Player sees | Fix | Cost | Risk |
|--|--|--|--|--|--|
| 1 | Red blocks on clothing | Obvious corruption | PLAN 1.5 then 4.3 (decode/format audit) | ~0 ms | low |
| 2 | Texture slots exhausted, 12k waiting, 8k unmapped | Blurry/missing textures | PLAN 4.5 (GPU detile, DirectStorage); raise slot budget (new item, check the so_far=45000 constant) | saves 2-5 ms | med |
| 3 | lodScale 0.5, grass off, texLod Mat 1.0 | Low LOD, pop-in, bare ground, softer mips | settings, free profile below | +1-4 ms GPU est. | low |
| 4 | Water reflection 256x128, shadow toggles | Blocky reflections, missing particle/tinted shadows | settings | +1-3 ms est. | low |
| 5 | Resolution/FSR | Soft image | fsrQuality 3->2 (A/B only; it was neutral on frame time, host-bound) | ~0 measured | low |
| 6 | Builder off: GI, probes, reflections | Flat, black, no bounce light | VICTORY_BVH_BUILDER=1; needs the memory-overwrite fix (PLAN 4.4), then track B (3.1) | +40 ms now, ~15-20 after 3.1 | high |
| 7 | Shadow trace programs | RT shadows | 5 shaders do not translate (descriptor by computed index): PLAN 4.1 | 1.6 ms | high |
| 8 | 4 untranslated shaders, wave64 reductions (108 of 25,694 programs) | Small missing effects | PLAN 4.1, 4.2 | small | med |

Frame time is host-bound (copies, copy-back), so GPU-side settings cost less on screen than ms suggest; ms figures marked est. are not measured.

## (1) Release-look profile (one item per run, one screenshot each, back up settings.xml before each; start from current file)
1. texLodScale_Mat_High 1.0 -> 0.25 and Mat_Med 1.0 -> 0.5
2. lodScale 0.5 -> 1.0
3. grassLod 0.0 -> 1.0
4. rtDynamicShadows false -> true (expect no change or drops; builder off)
5. shadowAlphaTintedShadows -> true
6. shadowParticleShadows -> true
7. waterReflectionResX/Y 256x128 -> 512x256
8. fsrQuality 3 -> 2 (A/B)
9. motionBlur false -> true (taste only)
10. Last: set VICTORY_BVH_BUILDER=1 (env, not a file), screenshot; expect corruption until 4.4.
Leave resolutionFullscreen 854x480 (reported value, render targets follow fsrQuality) and the frame caps (30/vSync 2 are the console cap, not fidelity).

## (2) Order
Red blocks (1), settings 1-3 together as separate runs (3), texture slots/uploads (2), water and shadow toggles (4), then GI (6), shadow traces (7).

## (3) Blocked on code
- Items 1, 2: PLAN 1.5, 4.3, 4.5 (the slot budget has no plan item: new).
- Item 6: PLAN 4.4 (builder correctness), then 3.1 (cost); gated on 1.4 (PIX timings).
- Item 7 and 8: PLAN 4.1, 4.2.
- Settings 1-9: nothing blocked.
