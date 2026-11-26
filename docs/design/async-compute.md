# Async compute: the title's compute queue on a host compute queue

This is a design, plus one measuring tool that was built: the card meter's compute-list line
(section 6, branch `pass-hash`). Nothing was run for this document. Tree read: `wt-passhash` at
fast-integ `2261b969`. Files read: `host.rs` (the one host queue at ~934, `card_before`,
`HostList::close`), `queue.rs` (`execute`, `wait`, `Held`, "the host picks engines itself"),
`queue2.rs` (the copy lanes and their rules), `fence.rs`, `pipefence.rs`, `cardmeter.rs`. Inputs:
`card-budget.md` rank 11 and §5, `submit-split.md`, `research-gpu-driven.md` 5E.

Labels: **[M]** measured in a committed log (source named), **[C]** read in code, **[E]** estimate
with its reasoning, **[?]** unknown until the named run.

## 0. Summary and verdict

- **Today.** Every title queue (graphics and compute) lands on **one host direct queue**
  [C, host.rs ~934; queue.rs `create_x`]. A title queue waiting on a fence is **held on the CPU**
  (`Held::Wait`) until the signal's flight has landed [C, queue.rs `wait`]. The title's compute
  work therefore never overlaps its graphics work. On the console it does.
- **The prize.** card-budget estimates 3–8 ms of card time [E]. That is an upper bound on overlap.
  It assumes a meaningful share of the title's ~46 ms of list time [M, run 34] is on
  compute-type lists, and that it can run beside graphics work that leaves the card's units free.
  Both are **[?]**. The title's card time is mostly compute *dispatches* and indirect commands
  [M, card-budget §1], but dispatches in **direct** lists gain nothing from a second queue. Only
  lists the title submits to a compute queue are candidates.
- **The risk.** It is mostly fence ordering, and it is spread over our own machinery, not the
  title's. The send-and-hash list, the compare list, copy-back, the relay's landing,
  `QUEUE_ORDER`/queue2 numbering, pipefence tickets ("one queue: must stay 0"), and each host
  list's resource-state tracking all assume **one total order on the card**. Each needs a rule.
  Getting one wrong gives a hang (a wait nothing meets), a device removal (a state illegal on a
  compute queue), or stale bytes (copy-back before the compute flight ran).
- **Verdict.** **Measure now; build only once the card is the wall, and only if the measured
  compute-list time is at least 6 ms a frame.**
  - While the submit thread is the wall (today: ~118 ms of CPU against ~74–80 ms of card
    [M, submit-split §1, card-budget §1]), async compute moves the frame by 0 ms.
  - After card-budget ranks 1–8 the card floor is ~35 ms [E]. Then 3–8 ms is a real share. Even
    so it ranks after the resolution test (rank 9), because the effort is weeks and the failure
    modes are hangs.
  - The tool to measure it is built (section 6). One diagnostics run answers the size question.

## 1. How queues work today [C]

| piece | behaviour |
|---|---|
| Host queues | One `D3D12_COMMAND_LIST_TYPE_DIRECT` queue, plus queue2's optional copy lanes (`VICTORY_QUEUE2` bits 1/2, ours only) |
| Title queues | Made as asked (`CreateCommandQueue[X]`). The kind is kept (`queue(this).kind`), but every submission goes to the one host queue. |
| `ExecuteCommandLists` | `execute_now`: cut/split at waits, held behind unmet fences, else `Host::submit`. This prepares (`card_before`, `run_deferred`), sends, and pushes to the relay line. |
| Title `Signal` | Set when the flights before it **land** (the relay, `settle`), never early |
| Title `Wait(fence, v)` | If `completed(fence) < v`, the queue's later submissions are **held on the CPU** until a signal meets it |
| Our lists | Sync (send-and-hash) **before** a submission's lists, Check (compare/fetch) **after**, copy-back by the CPU after landing. All on the direct queue, in order. |
| Resource states | Each host list tracks texture states from a global state, with a prelude list for transitions at submission. This is correct only because one queue runs everything in submission order. |
| queue2 (precedent) | Copy lanes wait only on fence values **already signalled/sent** ("each wait's partner lies ahead of it in its queue"). Landing buffers keep a lane from writing what the direct queue's work in the air names. Only buffers go through a lane. |

queue2's rules are the template. They show that the project already runs a second host queue
safely, but only for buffers, and only with waits on values already sent.

## 2. Mechanism

### 2.1 Queues and fences

- One **host compute queue** (`D3D12_COMMAND_LIST_TYPE_COMPUTE`) and one host fence for it, beside
  the direct queue and its fence. Title queues of the compute kind send there. Title copy queues
  stay on direct, as today.
- A title list of the compute type is recorded into a **compute-type host list**. Today all host
  lists are direct-type. A compute list cannot record graphics state, which a title compute list
  never has. The pools (`IDLE`) are split by type.
- **Cross-queue waits.** When title queue Q waits for fence F at v:
  1. If the submission whose landing will signal F≥v has **already been sent** to host queue H,
     translate the wait into `ID3D12CommandQueue::Wait(fence(H), sent_value)` on Q's host queue,
     and send Q's submission now. No CPU hold.
  2. Otherwise, hold on the CPU as today.

  This is the same no-deadlock rule as queue2: a GPU wait only ever names a value already
  submitted on the other queue.
- **Title signals.** They are set when *all* host queues' flights before the signal point have
  landed. Each title submission records `(queue, host fence value)`. `settle` checks the fence of
  the queue it was sent on. Because a Signal is ordered after that queue's own earlier work (the
  title's semantics), this is enough.

### 2.2 Our own work around a compute submission (the hard part)

| our step | today | with a compute queue (stage A1, conservative) |
|---|---|---|
| Sync list (`card_before`: changed bytes up, hash) | direct queue, before the submission | **Still on direct.** The compute submission first `Wait`s for the direct fence value behind that sync list. |
| Fills, scratch uploads, clamp passes inside the list | in the list | Same, in the compute host list. Clamp passes are compute (legal). Scratch is per list. |
| Check list (compare / fetch of written pieces) | direct, after | **Still on direct.** Direct `Wait`s for the compute fence value of that submission first. That serialises direct behind compute at every check, which is the main overlap loss in A1. |
| Copy-back (CPU) | after the landing of the direct fence | After **both** fences reach the submission's values. The relay keeps `(queue, value)` per flight. |
| `QUEUE_ORDER`, queue2 `order_done` | numbers sends on one queue | Numbered per host queue. A fetch merges against the sends of **both** queues that it has waited for. |
| pipefence tickets (`VICTORY_PIPE_FENCES`) | one fence, out-of-order counter "must stay 0" | Ticket = a vector `(direct value, compute value)`. Retire when both are reached. The out-of-order counter checks each queue on its own. |
| Card meter, passtime | timestamps per list; one frequency | Timestamps work on compute queues, but **frequencies are per queue** (`GetTimestampFrequency` on each). Card busy becomes the union of intervals, not the sum (card-budget §5 item 4). |

Stage A1's overlap therefore comes from a compute flight running **while the next direct flight
runs**: the title's own pattern of a compute queue working beside graphics. It does not come
from our lists.

**Stage A3** narrows the waits so they cover only real dependencies:

- the compute flight waits only for sync lists that touched the copies its lists bind;
- the direct check waits only when the compute flight declared GPU writes.

The declared ranges already exist (`Shadow::gpu_writes`, `card_before`'s `Step::GpuWrite`
ranges).

### 2.3 Resource states on a compute queue

- A compute queue may only use compute-legal states: `UNORDERED_ACCESS`,
  `NON_PIXEL_SHADER_RESOURCE`, `COPY_*`, `COMMON`, `INDIRECT_ARGUMENT`. A texture in
  `RENDER_TARGET`, `DEPTH_WRITE` or `PIXEL_SHADER_RESOURCE` when a compute list uses it is an
  error, and a device removal on some drivers.
- **Buffers**, including all our copies, are promoted from and decay to `COMMON` across queues.
  This is the queue2 argument and holds here.
- **Textures** do not decay unless they are simultaneous-access. Rule for A1:
  - the compute queue's prelude moves every texture its submission names from wherever the
    global tracker says into a compute-legal state;
  - that prelude runs **on the direct queue** before the cross-queue `Wait`, because direct
    can transition from graphics states;
  - after the compute flight, the next direct use transitions as usual. The tracker records
    the compute-legal state the compute list left.

  This is one more barrier list per compute submission. Depth textures and their colour twins
  (`twin_resource`) need the twin rule kept. Enhanced barriers with `COMMON` layouts are the
  later, cleaner form.
- **Indirect argument buffers** used by compute lists' ExecuteIndirect are buffers (fine).
  ExecuteIndirect of **draws** cannot run on compute; the title's compute lists only hold
  dispatches [?]. If a draw appears in a compute-type list, log it and fall back for that
  submission.

### 2.4 Translator and shader cache

- **No translator change.** Compute pipelines are the same objects, and a compute PSO can run
  on either queue type. Root signatures are unchanged. `WaveSize`, wave64 handling and
  `translation_fingerprint()` are untouched. **The shader cache key does not change**, and
  the pipeline library is shared.
- One check: our own compute programs (clamp, hash, compare, scatter) must not rely on
  graphics-queue-only features. They are compute PSOs, so they are fine [C].

## 3. Hazards, ranked by how bad a miss is

1. **Hang: a GPU wait on a value never sent.** Prevented by construction: GPU waits only for
   values already sent (§2.1). Detector: a watchdog that logs any host queue not progressing for
   2 s with its waits, and `VICTORY_ASYNC_COMPUTE=alt` falls back on detection.
2. **Device removal from an illegal state on the compute queue.** Prevented by the direct-queue
   prelude (§2.3). Detector: the debug layer in a WARP soak (the queue2 precedent caught its bug
   there), and DRED on the first card run.
3. **Stale title bytes: copy-back or a check before the compute flight ran.** Prevented by
   per-flight `(queue, value)` and waiting on both. Detector: `VICTORY_REFRESH_CHECK=1`,
   `fill_check`, and the check list's mismatch counters must not move against the control.
4. **Early or late title signal.** A signal must follow the landing of its own queue's
   earlier work. Early is a correctness bug (the title reads results not yet written). Late is
   only lost overlap. Detector: pipefence's early-signal counter, kept per queue.
5. **Write-write between queues on one copy.** The title orders this with its fences, so a
   correct title never writes the same range from both queues without a fence. Our scatter and
   upload into copies stays on direct, behind which compute waits (A1).
6. **Lost overlap.** A1's waits around Sync/Check may serialise most of it. This is not a
   correctness risk. It is the reason the stage gate is measured card-busy-as-union, not
   card-busy-as-sum.

**Fence-ordering risk overall: high in effort and debugging, low in silent-corruption risk** if
the rules above are followed. Every rule has a detector already in the tree or cheap to add.

## 4. Staged plan

| stage | switch | what | est. card ms [E] | effort | gate / test |
|--:|---|---|---:|---|---|
| A0 | none (card meter, on in diagnostics) **built** | Card time of the title's compute-type lists, every 256 frames | 0 | done | One run. If the compute lists are **< 4 ms a frame**, stop here. |
| A0b | `VICTORY_ASYNC_COMPUTE_DRY=1` (new, S) | Counters only: cross-queue waits per frame, and how many would be GPU waits (signal already sent) vs CPU holds; textures named by compute lists and their states | 0 | S (a day) | Predicts A1's overlap: many CPU holds means little to gain |
| A1 | `VICTORY_ASYNC_COMPUTE=1\|alt` | Host compute queue; compute-type host lists; GPU waits per §2.1; Sync before and Check after on direct, with cross-queue waits; direct-queue prelude for textures; per-queue relay values; pipefence and cardmeter per queue | **−1 to −4** (overlap with conservative waits) | L (2–3 weeks) | WARP soak with the debug layer, then one card run with DRED. Card busy as a union against the control. Mismatch and refresh-check counters unchanged. Watchdog silent. Picture checks. |
| A2 | (instrument) | Card busy as a union of both queues' intervals in the frame log; per-queue card ms | 0 | S | Needed to read A1 at all |
| A3 | `VICTORY_ASYNC_COMPUTE=2` | Narrowed waits: compute waits only for sync lists that touched its copies; direct's check waits only if compute declared writes | **−1 to −4 more (total −3 to −8)** | M | As A1. Plus a unit test of the wait planner over recorded submissions. |

Frame effect: **0 while the CPU is the wall.** At the card floor (~35 ms [E, card-budget §4]),
A1 + A3 give −3 to −8. That matches card-budget's rank 11 estimate, which assumed this
narrowed form.

## 5. Weighing it

| | for | against |
|---|---|---|
| Size | Up to 3–8 card ms, ~10–20 % of the floor; nothing else on the list overlaps work | Bounded by the compute-type list share [?]. A0 answers it in one run. |
| When it pays | Only once the card is the wall | Today it is not (CPU 118 ms vs card 74–80 ms) |
| Effort | Translator and shader cache untouched | Touches 6 subsystems' ordering assumptions (relay, queue2 numbering, pipefence, cardmeter, state tracking, copy-back) |
| Failure modes | Mostly loud (hang, device removal), detectable in a WARP soak | Hard to debug when they happen on the card only |
| Alternatives at the same card ms | Rank 9 (1080p render) is a no-code test worth 5–12 ms; ranks 4, 6 and 8 are days each | — |

**Recommendation.** Run A0 (built) in the next diagnostics run, and A0b when convenient.
Schedule A1 only after card-budget ranks 1–8 have landed and the frame log shows the card as
the limit, and only if A0 shows ≥ 6 ms. Until then, PIPE_AHEAD=2 (two frames in flight on one
queue) gives the "card busy = frame" effect with none of these ordering changes.

## 6. The measuring tool (built, branch `pass-hash`)

- `cardmeter.rs`: `mark_compute(pair)` records which host lists record a title list of the
  compute type. `HostList::meter_compute` is called from `list.rs` `close` and at a cut at a
  wait, before the list's meter pair ends.
- Every 256 frames, after the "card time of the title's lists by what they bind" table:

```
card time of the title's compute-queue lists (on our one direct queue; what async compute could overlap at most): X ms a frame in Y lists, of Z ms in all the title's lists
```

- On in every diagnostics build with the card meter on (the default). No new switch. Cost: a
  set insert per compute list.
- The pass-hash line (`VICTORY_PASS_HASH=1`, `bindless.md` §7) also gives the CPU-side share:
  "compute lists P passes C calls" per frame.
