<h1 align="center">📐 Designs</h1>

<p align="center"><i>The plans for the big changes, written before the code.</i></p>

<p align="center"><a href="../../README.md">← Back to Triumph</a> · <a href="../research/README.md">Research</a> · <a href="../tasks.md">Task log</a></p>

---

Every design lands in steps, each behind its own `VICTORY_*` switch that is off by default. Each step
is measured against the previous one (fps · ms · draws · dropped, plus a screenshot) before the next
begins.

| | design | what it changes | expected | effort | status |
|:--:|:--|:--|:--|:--|:--:|
| 🔗 | [**Wait hunt**](wait-hunt.md) | Every remaining reason the submit thread waits and only one frame is in flight; the false fill dependency (one miss makes every fill wait) | est. −5 to −10 ms; unblocks ordered copies | days | 🔄 measuring |
| 🔬 | [**Shader quality**](shader-quality.md) | Narrow passes on our emitted DXIL: the console legacy multiply (19 % of all our DXIL), signed-zero min/max, exact rcp, f16 | est. −3 to −7 ms card | days | 🔄 building |
| 🎚️ | [**Settings route (native, RT on)**](settings-route.md) | Game settings and resolution runs for 30 fps with ray-traced lighting on and no DLSS | est. −22 to −38 ms needed from trims + Track B | days of runs | ⏳ runs planned |
| 🧠 | [**Card-authoritative memory**](card-authoritative.md) | The card's copy is the master; CPU pages fault in on first touch | est. −6 to −14 ms first stage; programme −40 to −90 | weeks | 🔄 building |
| 🎞️ | [**Frame generation**](framegen.md) | Steady presenter, then open-source interpolation; optional user-supplied DLSS-FG | displayed fps only, once real fps ≥ ~16 | weeks | 🔄 presenter built |
| 🧷 | [**Bindless and pass replay**](bindless.md) | Buffer pull-model, draw runs, per-pass replay (gated on a measured hit rate) | est. 0–3 ms now, 7–22 ms per recorder near 33 ms | weeks | ⏳ tool built |
| ⚙️ | [**Async compute**](async-compute.md) | The title's compute queue on its own host queue | est. −3 to −8 ms card, 0 while CPU-bound | weeks | ⏳ deferred |
| 🧮 | [**Card time budget**](card-budget.md) | Where the card's ~80 ms a frame goes and how to cut it | wide card hash est. −24 to −30 ms card; clamp/barrier batching −6 to −12 | days–weeks | 🔄 building |
| 🧵 | [**Splitting the submit thread**](submit-split.md) | Present ticket, wide prepare, send thread, list digests | est. −30 to −60 ms submit combined (overlapping) | days | 🔄 building |
| 🧾 | [**Recording cost**](recording-cost.md) | Recorder locks, dedupe bugs, bind elision, constant buffers in place (port 5) | est. −8 to −15 ms in the control | days | 🔄 building |
| 🛡️ | [**Cheaper write tracking**](tracking-cost.md) | Guard epochs, one guard pass per submission, hot blocks, page-kind map | est. −10 to −12 ms combined | days | 🔄 building |
| 🧠 | [**Zero-copy memory (UMA)**](uma.md) | Title memory used by the GPU directly, so the per-frame copy, hash and copy-back machinery goes | −35 to −65 ms submit thread, −20 ms card; ends card-bound at ~50–55 ms | weeks | ⏳ next big item |
| 🖼️ | [**DLSS in place of FSR**](dlss.md) | Hook the title's FidelityFX loader (5 exports) and drive DLSS from the same inputs | GPU time down 10–20 ms in performance mode; little fps until zero-copy lands | days | ⏳ after UMA |
| ⚡ | [**Per-lane and native wave ops**](per-lane.md) | Native quad ops, folded lane indices, reduction ladders and flat regions in the translator | a few ms of card time; settles the 4–8× question first | weeks, staged | ⏳ |
| 🎮 | [**Debug menu**](debug-menu.md) | In-game overlay, live switches, shader-skip picker, viewers; later teleport and spawning | testing speed, not fps | 3–4 weeks (layer tools) | 💤 after stable 30 fps |
| 🎮 | [**Developer menu v2**](devmenu.md) | Build-ready: overlay renderer, F10 input gate, live-switch registry, then world tools (discovery, native logger, game-thread queue, crash containment, per-build tables) | testing speed | A: 3–4 wk · B: 6–8 wk | 💤 after stable 30 fps |
| 🧵 | [**Frame pipelining**](pipelining.md) | Per-frame fence tickets → 3 buffers + waitable swapchain → 2–3 frames in flight; copy-back compare-and-swap first | ~127 → ~75–85 ms; with UMA ~55–60 | days–weeks | 🔄 stage 1 building |
| 🎯 | [**Indirect overhead**](indirect-overhead.md) | Per-copy gather, barrier merge, one-record clamp, dispatch windows | 6–10 ms GPU | days each | ⏳ |
| 🧱 | [**GPU textures**](gpu-textures.md) | Mip-level upload, GPU detile, DirectStorage straight to the card, red-block triage | 3–9 ms + streaming backlog | days–weeks | ⏳ |
| ☀️ | [**Ray tracing**](raytracing.md) | Track B: faster exact emulation (page lookup per node, early exits); track A: native DXR/RayQuery, gated | traces ~24 → ~5–8 ms | B: weeks · A: months | ⏳ B first (owner's call) |
| 🧪 | [**Experiment harness**](harness.md) | Overnight run queue, contention guard, screenshots, on/off phase split, auto rows | measurement throughput | days | ⏳ (phase split done) |
| 🖼️ | [**Picture checks**](picture-checks.md) | Black/flat/saturated-block detectors, RenderDoc shader-swap A/B, artefact bisect | catches wrong rendering | days–weeks | ⏳ |
| 🪟 | [**UI planes**](ui-rendering.md) | The title presents game + UI planes; we showed plane 0 only (HUD/menus dropped). Compositor ported behind `VICTORY_UI_PLANES` | restores HUD/menus, ~0.3 ms | days | 🔄 built, in-game test running |
| 💾 | [**Game saves**](game-saves.md) | Saves work on disk; harden: checksums, keep-old-until-valid, quarantine, real quota, call logging, backups | durability | days | ⏳ |
| 🏠 | [**Main menu**](main-menu.md) | Trace the menu's calls, add the missing dialogs and save calls, boot-to-menu mode | a real Continue/New/Load/Settings menu | days | ⏳ |
| 🚀 | [**Launcher**](launcher.md) | Rust egui app: setup/validation, profile picker, switches, cache warm-up, saves, logs | usability | v1 1.5–2 wk | 💤 parked |

## How the big one lands

```mermaid
flowchart TD
    S0["0 · census<br/>which memory is read back"] --> S1["1 · no copy-back<br/>except an allowlist<br/><b>−35 to −40 ms</b>"]
    S1 --> S2["2 · drop the hash lists<br/><b>−19.8 ms card</b>"]
    S2 --> S3["3 · section heaps<br/>copies 23.5 → ~8 ms"]
    S3 --> S4["4 · bindings through<br/>a region table"]
    S4 --> S5["5 · page directory<br/>filled at map time"]
    S5 --> S6["6 · upload-heap writes"]
    S6 --> S7["7 · direct ReBAR<br/>if a probe passes"]
```

Licensing notes are in each design. In particular, DLSS is a closed SDK and this repository is
GPL-3.0, so how it ships is the owner's decision. The X reference layer (GPL-2.0-only) is read for ideas only, never copied.
