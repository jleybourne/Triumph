# GPU time

Where the card's **90.8 ms a frame** goes, measured by the layer's own `cardmeter`, which brackets each
list with timestamps and reports the card's time by what the list binds. The figures below are one
settled world frame, 256 frames averaged, with the page-pool change on (frame 125 ms, 8.0 fps).

## The answer: it is diffuse

```
the title's lists        63.3 ms a frame in 211 lists of 93 kinds
largest single kind       6.79 ms   (7 compute pipelines)
then                      4.13 · 3.36 · 3.17 · 2.95 · 2.81 · 2.70 · 2.70 ms
remainder                 ~60 kinds under 2.5 ms each
```

**No single pass, list or shader holds more than about 7 ms of the 90.** The largest kind is 7.5% of
the card's time; the ten largest together are under 40%. Cutting the GPU in half therefore cannot be
done by removing one hotspot — it needs every draw to get cheaper, which is shader instruction cost,
or fewer lists, which is batching work the title's own command stream decides.

Two corollaries that have been measured separately and are worth stating together:

- **Fill rate is not the constraint.** Dropping internal resolution to 540p saved 4% (`runs.md` 6).
- **The cost is not per-draw API overhead.** The real barrier count is 706 a frame, 0.05 a draw
  (the 13,800 an agent reported was fabricated — its census printed zeros). A dedupe of barriers
  cannot pay at that count.

## What would move it

1. **Cheaper translated shaders.** The census counts 1,433 programs and the top 20 carry about 48k
   instructions; translated code runs 4-8x the console's instruction count. This is the only lever
   that acts on every one of the 93 kinds, and it is instruction-level work in the translator
   (`docs/per-lane.md` covers the wave-reduction part of it).
2. **Fewer lists.** 211 lists a frame against 4,500 draws means small batches; batching is decided by
   the title's command stream, so the layer can only help where state is provably unchanged.

## Instrument note

`cardmeter` is the trustworthy instrument here and it runs whenever the diagnostics profile is built.
A separate `passtime` module was added by an agent to bracket three spans with timestamp queries; it
is wired and called but has printed no line in any run, so it is not a source for anything on this
page. The per-draw census added in the same stretch printed zeros for every counter: neither is used.
