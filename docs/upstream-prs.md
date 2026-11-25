# Upstream pull requests worth taking, and why they do not apply yet

The watch over the local clones names roughly a hundred open pull requests a
day. These are the ones whose subject is a line this tree actually measures,
with what was tried.

All of them are authored against upstream `main`, which is about 531 commits
ahead of this tree's base. Cherry-picking one lands its unrelated
prerequisites in the same hunks, so a mechanical take is a rewrite, not a
pick.

## Tried

**victory #2210** — eliminate O(N^2) linear scans in `Lower` and
`resource::track`. The most valuable of the set for this tree: recording is
about 265 ms a frame and those scans are inside it.

`git fetch mv pull/2210/head` then `cherry-pick -n` gives five conflicts in
`crates/victory-shader/src/emit/lower.rs` and two in
`crates/victory-shader/src/resource.rs`. They are not whitespace: upstream
has since replaced how `x.io.interps` and `x.res.scalar` are built (a
prebuilt `exports` value, a precomputed `mask_number`), so taking the PR
means taking those upstream changes with it. Rebased onto this tree it would
be a genuine port.

## Not tried yet

- **#2217** per-draw/dispatch state calls dropped, `ID3D12GraphicsCommandList6`
  cached, a doubled `shadow::find` removed. Aimed at the same 265 ms.
- **#2211** the 60,000-pipeline startup allocation storm, a doubled BLAKE3
  hash, `USED` lock contention.
- **#2216** `HostList::keep` pointer-deduplicated, capacity kept across
  resets, `Host::execute` and `used_pipeline` sped up.
- **#2209** dead channel loads skipped in `format_load`, several loads
  vectorised.
- **#2176** a Wave32 host translation mode for console wave64 programs. That
  is the direct fix for programs refused for wanting 64 lanes on a 32-lane
  card; this tree refuses none today, so it is insurance rather than a gain.
- **#2161** releases idle placed textures sooner past 90 percent of the
  budget. This tree caps residency at 60 percent, which is what removed the
  spill; the PR matters if that cap is ever raised.

## The way to take any of them

Rebase the PR onto this tree, not cherry-pick it: fetch the branch, make a
work copy, drop the upstream refactors that are not wanted, and keep the
scans-to-maps change alone. Then measure recording and the world frame
against `docs/frame-budget.md`, several runs, medians.
