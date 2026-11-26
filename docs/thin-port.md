# The THIN port — what it is and exactly what is left

`VICTORY_THIN` is the one mechanism in the other fork that changes the shape of our worst cost. This
file is the brief for finishing it; the three source files are already in this branch.

## Why

```
our frame                 20,293 list calls = 243.2 ms of work on the submitting thread
their mechanism           the same calls written down as data, translated on 8 threads
their measurement         36,000 calls = 230 ms over 8 threads; 75 → 68 ms of frame
```

243 ms is larger than the frame we are trying to reach. Nothing else in the layer is that size except
the card's own work.

## What is already in the branch

```
crates/victory-gpu/src/list/thin.rs     912 lines   copied, NOT declared in list.rs
crates/victory-gpu/src/queue/behind.rs 1,086 lines  copied, NOT declared in queue.rs
crates/victory-gpu/src/cores.rs          107 lines  copied, NOT declared in lib.rs
```

None of the three is `mod`-declared yet, so the tree still builds exactly as before.

## What is left, in order

1. **Declare the modules.** `#[cfg(windows)] mod cores;` in `lib.rs`; `pub(crate) mod thin;` at the top
   level of `list.rs`; `mod behind;` inside `queue.rs` (which is a file, not a directory — this works in
   Rust and is why `queue/behind.rs` is where it is).

2. **Add `queue`'s four helpers** that `thin.rs` calls: `line_on()`, `line_moved()`,
   `before_sync()`, `no_drain_here()`. In the other fork they are one-liners delegating to `behind`.
   Their definitions are at `queue.rs:442-465` in `carlm125/cool_fox`.

3. **Add `list::boxed()`.** The other fork refactored the list constructor into

   ```
   fn boxed(kind: u32, pipeline: This, thin: bool) -> Box<List>
   ```

   which chooses `THIN_FUNCTIONS` / `NEWER_THIN_FUNCTIONS` instead of `LIST_FUNCTIONS` when `thin` is
   true, and stores `thin: thin::Front` as a field of `List` (`#[cfg(windows)]`). Ours constructs the
   `Box<List>` inline in `new_list` — extract it, then gate on `thin::on()`.

4. **Drop the host list when thin.** Their constructor line is
   `host: crate::host::host().filter(|_| !thin).and_then(|h| h.new_list())`.

5. **Build, then fix what `thin.rs` assumes.** With the three modules declared, the first build will
   report the calls into our tree that do not exist. When the port was first attempted with only
   `thin.rs` declared, the errors were: `crate::cores::keep_many_on_fast_cores`, `crate::queue::line_on`,
   `crate::queue::line_moved`, and `super::boxed` — all four are addressed by steps 1-3.

6. **Measure it, and measure it honestly.** `VICTORY_THIN=1` on top of the confirmed launcher, then:

   ```
   D:\victory-work\scripts\run_viewplace.ps1        # baseline, no THIN
   D:\victory-work\scripts\run_play.ps1             # confirmed set, no THIN
   ```

   Compare **ms/draw and the list-call line**, not fps alone: `recording per frame (N frames): X list
   calls Y ms`. The claim to beat is 20,293 calls / 243 ms. Report fps · ms · draws · dropped.

## Traps already paid for

- **`VICTORY_THIN` needs `crate::host::host()`**: their `on()` is
  `wanted() && host().is_some() && queue::line_on()`. With no host, thin refuses and nothing changes —
  a run that "measures no difference" may simply not have engaged it. Check the engagement line first.
- **Do not batch it with other switches.** Tonight the all-on set crashed the title at startup and could
  not be attributed; one switch per run is the only way to know what did what.
- **One game at a time.** A live `victory` makes the next launch exit instantly and the parser then reads
  the stale log.
- The reference implementation is GPL-3.0 and this repository is GPL-3.0; still reimplement into our
  structure rather than pasting wholesale, because their `List` and ours differ.
