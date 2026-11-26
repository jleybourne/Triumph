<h1 align="center">✅ Task log</h1>

<p align="center"><i>What was done, what it measured, and what is next.</i></p>

<p align="center"><a href="../README.md">← Back to Triumph</a> · <a href="research/README.md">Research</a> · <a href="design/README.md">Designs</a></p>

---

Every outcome is a run in [runs.md](runs.md), reported as **fps · ms · draws · dropped**. Two runs of
the same build differed by ~7 % (scene variance), so a gain smaller than that is only real when it
was measured inside one run (`alt` switches, which flip on and off every 20 s).

## Done

| | task | outcome | evidence |
|:--:|:--|:--|:--|
| ✅ | **Refused compute pipelines** | 311 → 7 refused. The debug layer showed the shaders index the sampler heap, and an earlier "fix" had removed the flag allowing that. Draws 3,962 → 4,706, dropped ~478 → 344, at 3 % more frame time. | run 26 · `pipeline-samplers` `d1e1af34` |
| ✅ | **`fsrQuality=4` black screen** | Fixed by the same change, since its passes were among the refused pipelines. It renders now but measures no gain yet (the GPU isn't the wall). | run 27 · capture `fsr4-fixed-211141.png` |
| ✅ | **Shader census** | 25,636 of 25,694 programs emit DXIL (99.77 %). The 58 left, by reason: 40 descriptor chosen by control flow, 10 hardware SGPR input, 5 descriptor read as data, 3 lane masks. | `victory translate` over the full archive |
| ✅ | **Wave64 on 32-lane cards (part 1)** | 1 of the 7 refused wave64 shaders now runs at wave32 (pipeline created on the card). The other 6 need lanes to meet inside branches. | `wave64-split` `ace7a91f`, 221 shader tests pass |
| ✅ | **Decoder fix from cool_fox PR #6** | Operandless VOP1 and the EXEC destination of VOP3 CMPX are decoded (ported with credit). | `wave64-split` `3edc3e67` |
| ✅ | **AVX2 build (`target-cpu=native`)** | 7.9 fps · 127.0 ms · 4,839 draws against 7.3 · 136.6 · 4,706. Within run-to-run variance, so not yet proven. | run A |
| ✅ | **Copy-back piece cut underflow** | Found by a new test: a copy starting mid-piece could skip a thread's range in release builds. Fixed. | `fast-integ` `c8318f37` |
| ✅ | **Test suite builds again** | One wrong module path had kept `victory-gpu`'s tests from compiling. Four tests in untouched files fail and are noted. | `c8318f37` |
| ❌ | **Copy-back reguard** (`VICTORY_BACK_REGUARD`) | Inert: copy-back writes to write-watched memory, not guarded memory, so 0 blocks were kept. Needs rework for write watch. | run B |
| ✅ | **README, CLAUDE.md, runs.md** | Rebuilt around the measured frame account; corrections recorded. | `main` |

## In progress

| | task | state |
|:--:|:--|:--|
| 🔄 | **`VICTORY_ASYNC_PRESENT`** | Overlaps the title's frame with our submission. It exists and has never been measured; the in-run A/B is next. Research estimate: 127 → ~75–85 ms. |
| 🔄 | **Copy-back thread pool** (`VICTORY_BACK_POOL`) | Built and test passes; awaiting a run. |
| 🔄 | **One fetch per settle** (`VICTORY_BACK_FEWER_WAITS`) | Built; awaiting a run. Removes ~5 card waits a frame (12.4 ms measured in those waits). |
| 🔄 | **GPU event markers** (`VICTORY_MARKERS`) | Being written: every draw named by shader hash in PIX and RenderDoc. |
| 🔄 | **Capture trigger** (`VICTORY_CAPTURE_TOOL`) | Being written: RenderDoc or PIX capture of a chosen frame, or on F11. |

## Next

| order | task | why |
|:--:|:--|:--|
| 1 | Zero-copy step 1: no copy-back except an allowlist | 96 % of copied-back bytes are never read by the title. −35 to −40 ms estimated. |
| 2 | `fsrQuality` 1–4 GPU-time sweep, and the shader-stub floor run | Says how much of the card's time is pixels and how much is our structure. |
| 3 | Title settings and frame caps, one per run | The only lever that removes work rather than speeding it up. |
| 4 | Remaining 6 wave64 shaders and 58 refusals | The rest of the dropped draws. |
| 5 | Warm shader cache, preload the working set | Load time and stutter. |
| 6 | DirectStorage to the card; the red block artefacts | Picture quality and a few ms. |
| 7 | DLSS, native wave ops, Frame Generation | After the GPU is the wall. |
| 8 | Debug menu | After a stable 30 fps. |
