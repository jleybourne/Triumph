"""Summarise a VIctory diagnostics run log: fps · ms · draws · dropped.

    python scripts/measure/frames.py <victory-run-....log.err> [--min-draws 3000]

Reads the per-interval frame lines the diagnostics build prints
("N fps, M ms/frame (...), D draws/frame, dropped X ..."), keeps the ones
in the world (draws >= --min-draws, default 3000, so menus and loading
are left out) and prints the median and best frame with their draw and
dropped counts, plus the means. Report all four numbers together: a
faster frame that draws less of the scene is not a win.

Also warns when a compiler ran during the run if you pass --check-cpu and
the log's "usual frame" lines show the submit thread getting much less CPU
than its at-work time (the sign of contention from another process).
"""
import argparse
import json
import re
import statistics
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*m")
FRAME = re.compile(
    r"INFO victory_gpu::host: ([0-9.]+) fps, ([0-9.]+) ms/frame.*? ([0-9]+) draws/frame, dropped ([0-9]+) "
)
USUAL = re.compile(r"submit thread: at work ([0-9.]+) ms \(user ([0-9.]+), kernel ([0-9.]+)\)")


def flip_re(name):
    return re.compile(r"\(VICTORY_" + re.escape(name) + r"=alt\): (ON|off)\b", re.I)


def phase_stats(rows):
    """rows: (ms, fps, draws, dropped). Median, best, means; None when empty."""
    if not rows:
        return None
    rs = sorted(rows)
    med, best = rs[(len(rs) - 1) // 2], rs[0]
    return {
        "samples": len(rs),
        "median": {"ms": med[0], "fps": med[1], "draws": med[2], "dropped": med[3]},
        "best": {"ms": best[0], "fps": best[1], "draws": best[2], "dropped": best[3]},
        "mean_draws": statistics.mean(r[2] for r in rs),
        "mean_dropped": statistics.mean(r[3] for r in rs),
    }


def alt_report(a, tagged):
    """tagged: (ms, fps, draws, dropped, phase, cycle, since_flip, phase_index)."""
    kept = [r for r in tagged if r[4] is not None and r[6] > a.settle]
    out = {"alt": a.alt, "settle": a.settle, "phases": {}}
    for ph in ("on", "off"):
        out["phases"][ph] = phase_stats([r[:4] for r in kept if r[4] == ph])
    last = max((r[7] for r in tagged if r[4] is not None), default=-1)
    pairs = []
    for c in sorted({r[5] for r in kept if r[5] >= 0}):
        on = [r[0] for r in kept if r[5] == c and r[4] == "on" and r[7] != last]
        off = [r[0] for r in kept if r[5] == c and r[4] == "off" and r[7] != last]
        if on and off:
            pairs.append(statistics.median(on) / statistics.median(off))
    out["pairs"] = pairs
    out["pair_ratio"] = statistics.median(pairs) if pairs else None
    if len(pairs) < 3:
        out["verdict"] = "inconclusive"
    elif abs(out["pair_ratio"] - 1) < 0.03:
        out["verdict"] = "neutral"
    else:
        out["verdict"] = "faster" if out["pair_ratio"] < 1 else "slower"
    out["draws_warning"] = False
    on, off = out["phases"]["on"], out["phases"]["off"]
    if on and off and max(on["mean_draws"], off["mean_draws"]) > 0:
        d = abs(on["mean_draws"] - off["mean_draws"]) / max(on["mean_draws"], off["mean_draws"])
        out["draws_diff_pct"] = d * 100
        out["draws_warning"] = d > 0.05
    return out


def print_alt(out):
    print(f"alt {out['alt']} (settle {out['settle']} lines after each flip)")
    for ph in ("on", "off"):
        s = out["phases"][ph]
        if not s:
            print(f"{ph:<3}: no samples")
            continue
        print(f"{ph:<3}: samples {s['samples']}")
        for n in ("median", "best"):
            r = s[n]
            print(f"    {n:<7} {r['fps']:5.1f} fps · {r['ms']:6.1f} ms · {r['draws']:5d} draws · {r['dropped']:4d} dropped")
    if out["pair_ratio"] is None:
        print("on/off ms ratio: no complete cycles")
    else:
        print(f"on/off ms ratio: median {out['pair_ratio']:.3f} over {len(out['pairs'])} cycles")
    print(f"verdict: {out['verdict']}" + (" (fewer than 3 cycles)" if len(out["pairs"]) < 3 else " (3 % band)"))
    if out["draws_warning"]:
        print(f"WARNING: mean draws differ by {out['draws_diff_pct']:.1f} % between phases; a win that draws fewer draws is not a win.")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("--min-draws", type=int, default=3000)
    ap.add_argument("--check-cpu", action="store_true")
    ap.add_argument("--alt", metavar="NAME", help="split by the flip lines of VICTORY_<NAME>=alt (e.g. ASYNC_PRESENT)")
    ap.add_argument("--settle", type=int, default=1, help="frame lines dropped after each flip (with --alt)")
    ap.add_argument("--json", action="store_true", help="machine-readable output (with --alt)")
    a = ap.parse_args()

    frames, usual, tagged = [], [], []
    flip = flip_re(a.alt) if a.alt else None
    phase, cycle, since, pidx = None, -1, 0, -1
    with open(a.log, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = ANSI.sub("", line)
            if flip:
                fm = flip.search(line)
                if fm:
                    phase = fm[1].lower()
                    if phase == "on":
                        cycle += 1
                    since, pidx = 0, pidx + 1
                    continue
            m = FRAME.search(line)
            if m:
                fps, ms, draws, dropped = float(m[1]), float(m[2]), int(m[3]), int(m[4])
                since += 1
                if draws >= a.min_draws:
                    frames.append((ms, fps, draws, dropped))
                    tagged.append((ms, fps, draws, dropped, phase, cycle, since, pidx))
            u = USUAL.search(line)
            if u:
                usual.append((float(u[1]), float(u[2]) + float(u[3])))

    if not frames:
        print(f"no frame lines with >= {a.min_draws} draws (still loading, or not a diagnostics build?)")
        return 1
    if flip:
        out = alt_report(a, tagged)
        if a.json:
            out["samples"] = len(frames)
            print(json.dumps(out, indent=2))
        else:
            print_alt(out)
            print(f"samples {len(frames)} (world frames, >= {a.min_draws} draws)")
        return 0
    frames.sort()
    best = frames[0]
    med = frames[(len(frames) - 1) // 2]
    def row(name, r):
        ms, fps, draws, dropped = r
        print(f"{name:<7} {fps:5.1f} fps · {ms:6.1f} ms · {draws:5d} draws · {dropped:4d} dropped")
    print(f"samples {len(frames)} (world frames, >= {a.min_draws} draws)")
    row("median", med)
    row("best", best)
    print(f"mean    {statistics.mean(r[2] for r in frames):5.0f} draws · {statistics.mean(r[3] for r in frames):4.0f} dropped")

    if a.check_cpu and usual:
        ratio = statistics.median(cpu / work for work, cpu in usual if work > 0)
        print(f"submit thread CPU / at-work: {ratio:.2f}")
        if ratio < 0.6:
            print("WARNING: the submit thread was starved of CPU; was something compiling? This run is not a fair measurement.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
