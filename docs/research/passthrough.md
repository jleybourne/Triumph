# Passthrough candidates in the GPU layer

Read-only, 2026-11-26. Tree `D:\victory-work\wt-bisect\crates\victory-gpu\src`. Log: `D:\victory-work\logs\victory-run-20261126-220710.log.err`
(frame line at 11:41:02). Card-time table: `wt-bisect\diagnostics\timing-run-295s-2026-11-25.txt`.
**[V]** read in code or log. **[E]** my estimate, assumptions stated. Nothing run, built or edited.

## Verified facts

1. **"Descriptor records left to the draws" is already in, on by default.** `descriptor/host.rs:1825` and `:1907` skip a changed
   record whose slot a draw read through a table in the last 240 frames (`tabled_lately`, `TABLED_FRAMES`);
   `VICTORY_HEAP_SYNC_TABLES=1` turns it off (`:2231`). Log: 264,192 slots looked at, 11,253 mirrored, 108,689 left to draws,
   **16.38 ms a frame**. `sync_heaps()` runs in submission preparing (`host.rs:6041`), i.e. on the submit thread.
   The 280->124 ms gain is therefore banked. `docs/frame-budget.md` shows turning it off is slower (167 ms).
2. **The layer never sees console descriptors (T#/V#/S#).** `create_srv/uav/cbv` get D3D12-shaped desc words
   (`descriptor.rs:938-960`). `srv_desc` reinterprets them as `D3D12_SHADER_RESOURCE_VIEW_DESC`, changing only the format
   (`pc_format`, `host.rs:2072`). The title's heap holds 12-byte tagged records `[VIEW_TAG, index, generation]`. Creates are
   already mirrored at call time, except buffers and textures with no host copy yet, which wait for a draw.
3. **The wall is the submit thread**, not recording: usual frame 178 ms, submit thread at work 164 ms (preparing 83, card wait 60),
   recording threads 82-95 ms each. Recording is 392.7 ms over 37,856 calls and 5,073 draws.
   A per-draw saving moves wall time only once the submit thread drops below recording.
4. **Card time by command** (run 280 timing): 47 ms a frame; indirect 29.2 ms in ~4,350 commands, dispatch 10.7, direct draws 1.9
   (280). Almost all draws are `ExecuteIndirect`, already native, with the title's count buffer passed natively (`draw.rs:3566`).

## Candidates, ranked by wall-time value over effort and risk

| # | Candidate | Current cost | What blocks passthrough | Saving [E] | Effort / risk |
|---|---|---|---|---|---|
| 1 | **Per-draw CBV tables rebuilt** (`draw.rs:2833 tables`) | [V] 13,015 fresh tables a frame, 2.0 ms of slot search; every CBV table goes into `fresh`, each entry a fresh `make_at` and a deferred scratch fill of the data (`defer_fill`); copystat shows 14,468 title copies, 7.0 MB a frame. The rest (hash of the table, `Vec`s, creates) is unmeasured | The host wants CBVs 256-aligned, the console 16. The direct path in `make()` already exists for aligned CBVs, but `tables()` forces all through scratch. Unknown: why aligned ones are not exempt | 10-30 ms of recording-thread time if a large share is aligned (assumes ~6 entries a table, ~0.3-0.5 us a create). Wall: 0 now | Small to try (skip `fresh` when shadow address is 256-aligned, A/B switch); high if it needs a shader change to raw loads with a bias. Risk medium: per-draw data snapshots |
| 2 | **Indirect clamp passes and their waits** (`indirect.rs`) | [V] 2,066 bounded commands a frame, 7,763 records clamped, two global UAV waits each, plus 714 title barrier groups | A wrong or untranslated pass leaves garbage args, and a huge draw hangs the card. No proof args are safe. Counts: none. Count buffers: 0 in use | Card 6-10 ms realistic with the staged options in `design/indirect-overhead.md` (gather by copy, merge dispatch clamps, narrow waits); full removal ~14 ms at 5-9 us a pass. Wall: 0 until the card is the wall | Medium; removing clamps is high risk (device removal) |
| 3 | **Byte-copied descriptor sync** (fact 1) | [V] 16.38 ms on the submit thread | Console-only: the title memcpys descriptors, PC D3D12 has no such thing. No passthrough exists, only narrower scanning | 5-10 ms if records were scanned only where bytes changed (it rescans whole changed pages: 264k looked for 11k mirrored) | Medium; risk low (equality-checked) |
| 4 | **Buffer views by index pass** (`draw.rs:1695 heap_buffers`) | [V] 33,721 slots looked at a frame; frame line "heap buffers 65.8 ms" includes 2.6 ms of textures and the host-copy refresh calls. Split between view creation and refresh is not logged | Views are over shadow copies of title memory (address, offset bias, `buffer_view_ok`), made late on purpose | unknown; add a timer before estimating | Low value to touch before measuring |
| 5 | **CopyDescriptors** (`descriptor.rs:999`) | per slot byte copy then `mirror` -> `CreateXView` | Source may be unmade (waiting texture, buffer view) | 0.3-0.5 us a slot over native `CopyDescriptorsSimple`; ~1 ms wall-equivalent [E] | Small, low |
| 6 | **Root arguments and vertex/index views per draw** (`draw.rs:1208-1400`, `:1055`) | per draw: a fresh `Vec<u32>` of user data, 5-10 `Set*Root*` and `IASet*` calls; switches `VICTORY_DEDUPE_ROOT_ARGS`, `SKIP_VERTEX_REBIND`, `SKIP_TARGET_TRANSITIONS` exist, off, never measured | Views are already 1:1 except address translation to the shadow copy; our root signature is not the title's (user-data dwords for translated shaders) | ~1 us a draw [E], 5 ms thread, ~1 ms wall-equivalent. The sibling's 6-12 us a draw is a guess: the whole card is 49 ms for 182 lists | Trivial (flip switches, A/B), low risk |
| 7 | **Barriers re-derived** (`list.rs:683 host_barrier`) | transitions rebuilt from the list's tracked per-resource state (`host.rs:6832`), whole resource, split begins dropped; UAV and aliasing barriers already pass 1:1 | Host textures are separate resources with legal-state rules and twins; the title's states cannot be trusted | 0-3 ms [E] (a prior note: other fork measured near zero) | Low value |
| 8 | **Render targets, depth** (`draw.rs:797`) | one `CreateRenderTargetView` per `OMSetRenderTargets`, per list not per draw | Already native | <1 ms | none |
| 9 | **Clears, copies** (`list.rs:1259`, `:1090`) | `ClearRenderTargetView` / `CopyTextureRegion` pass through | Buffer copies are really host-copy refresh (copies 76.9 ms on the submit thread, not a translation cost) | covered by the UMA work, `research-native-paths.md` | n/a |
| 10 | **Queries** (`list.rs:1391`) | not run: occlusion answers 1M, timestamps fake | Real queries cost more | negative; a title might cull fewer draws | not recommended |

## Notes

- Sibling-fork reports in `D:\victory-work` (`descriptor-copies.md`, `indirect-cost.md`) contain pasted build output that was
  never run, and `gpu-per-draw.md` divides card time by draws. I did not use their figures.
- Highest wall value is outside this list: host-copy refresh and copy-back (76.9 + 58.4 ms, submit thread).
- Needed measurements: a timer on `tables()`, one on `heap_buffers` view creation, and `VICTORY_BARRIER_TEST` runs.
