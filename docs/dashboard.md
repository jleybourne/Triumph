# PalmVI dev dashboard

A local page that shows what the game is doing while it runs (and after it froze or crashed), replays finished
world runs, and shows the Switchboard team view with a counts-only "Publish run".

## Start it

1. Build the diagnostics binary: `cargo build --profile diag -p victory --features diagnostics` (and
   `cargo xtask agility` once).
2. Start the dashboard: `node tools/dash/dash.mjs` (Node 18+, nothing to install) and open
   <http://127.0.0.1:47801/>. Options: `--port 47801` (the page), `--game 47800` (the game's stream),
   `--open` (opens a browser), `DASH_AGENT=<handle>` (Switchboard handle for Publish).
3. Run the game with `VICTORY_DIAG_PORT=47800`, e.g.

   ```powershell
   tools\world-run.ps1 -Title <title.exe> -Minutes 8 -PadScript "A*16/5@20" -WorldFrom 110 `
     -OutDir $env:TEMP\victory-world-run\<name> -Env @("VICTORY_D3D12_DRED=1","VICTORY_DIAG_PORT=47800")
   ```

The dashboard reconnects by itself; it can be started before or after the game. **Replay** reads
`%TEMP%\victory-world-run\<run>\run.log` and `summary.txt`, so it works for a run that crashed.

## What is local and what leaves the machine

The page shows shader hashes, drop reasons and log text: that is fine on this machine and nowhere else
(`docs/asset-policy.md`). The game's stream and the dashboard listen on 127.0.0.1 only, and the dashboard refuses
requests whose Host or Origin is not local. Nothing is written to disk by either.

**Publish run** (Team tab) is the only way out. It builds a summary from numbers only (`tools/dash/sanitize.mjs`
`summaryText`), strips 16-hex hashes, `0x` values, file paths, file names and shader/technique-like identifiers,
and then **refuses** to post if anything hash-like, a path separator, an underscore identifier or any word outside
a small fixed vocabulary remains. Preview shows the exact text first; "Post to Switchboard" then runs
`measure fps` (world median, only if the run reached the world and is not stuck on the account picker),
`measure shaders` (percent translated) and `post`. Example:

    [run] PalmVI run: 480 s, world fps 1.2, frames 1497, draws per frame 7012, dropped per frame 0, untranslated 6,
    translated 97%, stalls 3 (longest 22 s), device removed 0, presses 16, warns 120 (RTX 4090)

The Team tab charts these posts (and other agents' free-form run posts: "725 s", "frame ~1497", "fps") over time,
read from the `switchboard-state` branch.

## Game side: `VICTORY_DIAG_PORT`

`crates/victory-gpu/src/diagstream.rs`, compiled only with the `diagnostics` feature on Windows. Without the
variable there is no layer, thread or socket. With it, a tracing layer (installed by
`victory_core::logging::init_with`) reads the log events the filter lets through (info and above by default;
`VICTORY_LOG=warn` would starve it), and a probe thread reads frame/draw/pipeline counters, VRAM (DXGI
`QueryVideoMemoryInfo`, largest adapter) and the working set every 100 ms. The HTTP thread serves snapshots built
from that state, so the stream keeps going while the render thread is stuck: a stall is "no new frame for 1 s
after the first frame", and its cause is the stall reporter's latest event (target `victory_gpu::stall`, #64),
else a guess from the pipeline queue.

Endpoints (127.0.0.1:`VICTORY_DIAG_PORT`):

* `GET /events`: `text/event-stream`. `event: snapshot` about every 250 ms, `event: ev` for each event (the last
  256 are replayed to a new client first). At most 8 clients.
* `GET /snapshot`: one snapshot, `application/json`.

Other code can publish a number with no log line of its own via `victory_gpu::diagstream::gauge("name", value)`
(e.g. a descriptor heap fill counter); it appears under `gauges`.

## Schema 1

Numbers are JSON numbers (rounded to 3 places); a missing value is `null`. `t` is seconds since the stream
started. Fields from the host's 5-second frame-rate line are per frame, averaged over that line's frames.

```text
snapshot {
  schema: 1, t, seq                                   seq = id of the newest event
  frame:  { count, fps_live, draws_total, since_progress_ms }
  timing: null | { fps, ms, prepare_ms, copies_ms, copies_mb, textures_ms, textures_mb, gpu_wait_ms,
                   copy_back_ms, copy_back_mb, draws, dropped, being_made, indirect, at, present_ms (null) }
  pipelines: { queued, compute_waits, last_wait_ms }
  stall:  { active, since_ms, cause (string|null), cause_ms }
  stalls: [ { start, end (null = ongoing), frame, cause } ]          last 64
  drops:  [ { why, per_frame } ]                                      latest 5 s table, largest first
  untranslated: { count, translated, by_reason: [ { reason, count } ],
                  recent: [ { hash, stage, reason, t } ] }            reason has numbers masked as #; last 64
  heap:   { overflow: null | { index, count, sampler } }              "title slot past the host heap"
  memory: { vram_used_mb, vram_budget_mb, working_set_mb }
  copies: { submissions, title_copies, title_copies_mb, refreshes, refreshes_mb, skipped, runs, page_faults,
            watches, watches_ms, fallbacks, guard_faults }             copystat line (any may be absent)
  shadow: { <numeric fields of "shadow traffic per frame"> }
  direct_srvs: { slots, ranges, requested_mb, filled_mb, ms }
  warns:  { total, errors, top: [ { text, count } ] }                 text: message + fields, numbers as #, top 40
  pad:    { presses, script (string|null), last_key, last_at }
  device: { removed, dred_lines }
  gauges: { <name>: value }
}
ev { seq, t, kind, text, ...extra }
  kind: stall_begin (frame) | stall_end (seconds) | untranslated (hash, stage) | press (at) |
        device_removed | dred | error | wait (ms)
```

Change rules: adding a field is compatible; renaming, removing or changing a field's meaning bumps `SCHEMA` in
`diagstream.rs` and `SCHEMA` in `tools/dash/dash.mjs`, with a line here. The dashboard flags a schema mismatch.

## Known gaps

* `present_ms` is always `null`: Present is not timed separately yet (host.rs `Meter`).
* Descriptor heap fill is only the overflow event; a fill gauge needs a one-line `gauge` call in
  `descriptor/host.rs`.
* "Still on the account picker" is a heuristic: frames at menu pace 20 s after the pad script's last press and no
  shader translated for 30 s.
