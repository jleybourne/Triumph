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
import re
import statistics
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*m")
FRAME = re.compile(
    r"INFO victory_gpu::host: ([0-9.]+) fps, ([0-9.]+) ms/frame.*? ([0-9]+) draws/frame, dropped ([0-9]+) "
)
USUAL = re.compile(r"submit thread: at work ([0-9.]+) ms \(user ([0-9.]+), kernel ([0-9.]+)\)")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("--min-draws", type=int, default=3000)
    ap.add_argument("--check-cpu", action="store_true")
    a = ap.parse_args()

    frames, usual = [], []
    with open(a.log, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = ANSI.sub("", line)
            m = FRAME.search(line)
            if m:
                fps, ms, draws, dropped = float(m[1]), float(m[2]), int(m[3]), int(m[4])
                if draws >= a.min_draws:
                    frames.append((ms, fps, draws, dropped))
            u = USUAL.search(line)
            if u:
                usual.append((float(u[1]), float(u[2]) + float(u[3])))

    if not frames:
        print(f"no frame lines with >= {a.min_draws} draws (still loading, or not a diagnostics build?)")
        return 1
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
