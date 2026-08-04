# 03 — Upstream backlog triage

Status: investigation — no implementation committed by this document
Scope: decide this fork's policy on every upstream issue and PR not already
covered by `plans/00_static_build.md`, `01_resource_shape_bitflags.md` or
`02_search_paths_soundness.md`

## Goal

Close out the remaining `FloatyMonkey/slang-rs` backlog by producing a *decision*
per item — adopt, adapt, decline, or already-resolved-here — backed by an
investigation small enough to actually run. The point is that nobody has to
re-audit this list again; items that turn out to be non-issues for this fork get
that recorded, with the reasoning.

## Where things stand

As of the last sync (`89f82d8`), this branch contains every commit on
`upstream/main` and adds twelve of its own. Against upstream's six open issues
and six open PRs:

| Upstream item | This fork | Next action |
| --- | --- | --- |
| PR #37 — `c_char` not `i8` | ✅ same fix, `src/lib.rs:673` | none; conflict watch |
| PR #35 — bindgen optional, bindings checked in | ✅ gone further (`xtask`, no feature flag) | none; conflict watch |
| PR #30 — add CI | ✅ superseded, `.github/workflows/ci.yml` | none |
| PR #25 / issue #29 — static linking | ✅ superseded, `plans/00_static_build.md` | §5 — record why |
| Issue #23 — min Slang version | ✅ resolved by pinning | §4 — release-notes ritual |
| Issue #28 — resource shape bitflags | ❌ open | `plans/01_…` |
| Issue #31 / PR #34 — `search_paths` unsound | ❌ open | `plans/02_…` |
| **Issue #26 — objects outliving `Session`** | ❌ open | **§1 — investigate first** |
| PR #32 — user-defined filesystem | ❌ absent | §2 — decide on demand |
| Issue #1 — auto-generate vtables | ❌ open | §3 — verified blocked |

Everything below `Issue #28` in that table is what this document covers, minus
the two that already have their own plans.

## 1. Issue #26 — objects outliving `Session` segfaults

**Priority: highest.** This is the only remaining item that can corrupt a
consumer's process, and unlike #28 and #31 the fix is not obviously ours to make.

**What upstream found.** A twelve-comment thread
([#26](https://github.com/FloatyMonkey/slang-rs/issues/26)) with a reproduction
from HampusMat: a `ComponentType` (or the program linked from one) outliving its
`Session` segfaults inside `Slang::CompilerOptionSet::inheritFrom`. The diagnosis
converges on Slang-side refcount semantics — objects created from a `Session` do
not bump the `Session`'s refcount, so the `Session`'s own count hits zero and it
deletes itself while its children still point into it. laurooyen connected it to
[shader-slang/slang#6344](https://github.com/shader-slang/slang/issues/6344), the
same bug for `GlobalSession` vs `Session`, which Slang acknowledges as a bug with
no fix implemented. HampusMat reproduced it in C++, so it is not a binding
artifact.

**Why this fork's position differs.** Upstream builds against whatever Slang the
user installed and so cannot assume anything about the version. This fork pins
`2026.13.1` exactly (`justfile:7`, `slang-sys/static-release.sha256`) and can
move that pin deliberately. So the first question — untested by anyone — is
simply whether it still reproduces here.

**Investigate:**

1. Port HampusMat's repro from the issue thread into a test on this branch
   against the pinned static libs. Run it under
   `RUSTFLAGS=-Zsanitizer=address cargo +nightly test` as the issue does; a clean
   run is not proof of absence without the sanitizer.
2. If it reproduces: check whether shader-slang/slang#6344 has moved, and whether
   a newer Slang release fixes it — bumping the pin is a cheaper fix here than
   anywhere upstream can reach.
3. If it does not reproduce: determine *why* (a Slang-side fix between the
   version in the issue and 2026.13.1?) and confirm it with a regression test, so
   a future pin bump cannot silently reintroduce it.
4. If it reproduces and Slang has no fix: evaluate the fork-local wrapper fix —
   have each object hold a clone of the `IUnknown` for the `Session` that created
   it, so Rust's ownership keeps the `Session` alive. Touch points are the COM
   wrapper types in `src/lib.rs` (`Session`, `Module`, `EntryPoint`,
   `ComponentType`) and the `IUnknown` refcounting they share. Cost is one
   pointer plus an `addRef` per object; the risk is creating a reference cycle if
   Slang holds the reverse edge.

**Decision to produce:** one of *no longer reproduces (with regression test)*,
*fixed by pin bump*, *fixed fork-locally in the wrapper*, or *documented
limitation with a `# Safety`-style note on `Session`* — and if the last, say so
in the README rather than leaving it discoverable only by segfault.

## 2. PR #32 — user-defined filesystem implementations

**What it is.** [PR #32](https://github.com/FloatyMonkey/slang-rs/pull/32) adds
`src/fs.rs` implementing `ISlangFileSystem` from Rust: a hand-written vtable of
trampoline functions, refcounting on an `AtomicU32`, `queryInterface`/`castAs`
casting, and `loadFile` delegating to a user trait. `SessionDesc::file_system(my_fs)`
then lets Slang resolve `import` through arbitrary Rust code. +335 lines, tested
by the author on Linux and Windows, no review comments.

**This fork has nothing equivalent** — there is no `src/fs.rs` and no
`ISlangFileSystem` anywhere in `slang-sys/src/lib.rs`.

**Investigate:**

1. Is there a real need? The obvious ones are embedding shaders in the binary
   (which pairs naturally with this repo's whole static-linking thesis — a
   single self-contained executable with no `shaders/` directory beside it) and
   hot reload from an in-memory overlay. Check `Giesch/vulkan-slang-renderer` for
   whether either is wanted.
2. Review cost. This is unsafe FFI implementing a COM interface *outward* —
   Slang calling into Rust — which is a different and harder direction than
   everything else in the crate, where Rust calls into Slang. Refcounting bugs
   here are use-after-free. Budget real review time, not a skim.
3. Interaction with the manual vtables in `slang-sys/src/lib.rs` — the PR adds to
   `slang-sys/src/lib.rs`, which on this branch also carries the nine hand-written
   `*Vtable` structs. Check the merge is mechanical.
4. Note that this fork's `SessionDesc` is being restructured by
   `plans/02_search_paths_soundness.md` (owned storage, no `Deref`,
   `create_session` reading `desc.inner`). PR #32 also touches `SessionDesc`, so
   sequence #32 after plan 02 if both land.

**Decision to produce:** *adopt now*, *defer until a consumer needs it* (the
likely answer — the code will still be there), or *decline*. Do not adopt
speculatively; unsafe surface with no user is unsafe surface nobody tests.

## 3. Issue #1 — generate vtables automatically

**Verified state, so this needs no investigation to answer — only to re-check
later.** `xtask` already passes `.vtable_generation(true)`
(`xtask/src/main.rs:124`), and the result in `slang-sys/src/bindings.rs` is
exactly **one** generated vtable: `ISlangUnknown__bindgen_vtable`
(`slang-sys/src/bindings.rs:1427`), the one base interface with no inheritance.
Every derived interface is still hand-written in `slang-sys/src/lib.rs` — nine
`*Vtable` structs (`ICastableVtable:14` through `IModuleVtable:126`) holding
about 75 `unsafe extern "C" fn` pointers, in a 141-line file that is otherwise
just `include!`.

That is precisely the limitation upstream's issue names:
[rust-lang/rust-bindgen#2799](https://github.com/rust-lang/rust-bindgen/issues/2799),
vtables for inherited classes.

**Investigate (cheap, recurring):** on each bindgen upgrade, check whether #2799
has moved. If it has, the change is a config-only experiment: regenerate and see
how many of the nine structs bindgen can produce. Until then the answer is
*blocked upstream of us, by a Rust tool, not by Slang*.

**Decision:** record as blocked with the evidence above so it is not
re-investigated from scratch. Add a one-line note to `xtask/src/main.rs:124`
pointing at bindgen #2799, since `.vtable_generation(true)` currently looks like
it should be doing more than it is.

## 4. Issue #23 — specify the minimum Slang version

**Effectively closed for this fork, by construction.** The complaint is that
`main` could use a Slang API newer than the user's installed SDK, breaking their
CI. That failure mode requires the user to supply Slang. Here they cannot: the
version is pinned to `2026.13.1` in `justfile:7`, the archives are verified
against `slang-sys/static-release.sha256`, the libraries are vendored on release
tags, and `slang-sys/src/bindings.rs` is generated from the headers that ship in
that same archive (`xtask/src/main.rs:36-45` deliberately reads `vendor-local/`,
so headers and libraries cannot skew). A consumer pinning a git tag gets one
self-consistent set.

**The one residual action:** when the first release tag is cut, state the Slang
version in the release notes — see the "Upstream release — available" section of
`plans/00_static_build.md`, which already records `v2026.13.1-static` and its
hashes. Add "state the bundled Slang version" to the release ritual described
there.

**Decision:** resolved-here; no code change. Worth a note in the README's
Installation section that each tag bundles a specific Slang version.

## 5. Issue #29 / PR #25 — static linking

**Closed for this fork** by `plans/00_static_build.md`, though by a different
mechanism than either upstream proposal: vendored prebuilt static libraries from
`Giesch/slang`'s `release-static.yml`, not a `cmake`-crate source build (issue
#29) and not a feature flag over `SLANG_EXTERNAL_DIR` (PR #25). Static is the
only build mode this fork has.

Two things worth recording so they are not rediscovered:

**The unserved case.** Issue #29's author wanted "specific build flags or
versions of the slang library". Vendoring prebuilt archives does not serve that
directly — anyone needing custom flags must rebuild via `Giesch/slang` and re-pin.
That is a deliberate trade for reproducible, network-free, toolchain-free
consumer builds.

**HampusMat's blocker on PR #25 does not apply here.** He argued static linking
should wait because `slang-glslang` is always loaded dynamically
([shader-slang/slang#10652](https://github.com/shader-slang/slang/issues/10652)).
This fork's Slang build embeds it, and CI proves it on every run: the
`cargo test` step compiles to SPIR-V at `-O3`, which routes through the embedded
glslang wrapper (at `-O0` Slang emits SPIR-V natively and would not exercise it).
The comment in `.github/workflows/ci.yml` above the Test step records this — it
is the standing evidence, so do not "simplify" that test to `-O0`.

**Decision:** resolved-here; no action.

## 6. Merge-conflict watch

Two open upstream PRs overlap work already on this branch. Neither is a problem
today, but the next `upstream/main` sync will conflict if they merge:

- **PR #37** (`c_char`) — conflicts in `src/lib.rs` around `search_paths` and
  `push_strings`. Identical intent; take either side. Note that
  `plans/02_search_paths_soundness.md` rewrites that exact function, so if plan
  02 has landed the conflict resolves toward this fork.
- **PR #35** (bindgen optional) — conflicts in `slang-sys/Cargo.toml`,
  `slang-sys/build.rs`, `slang-sys/src/lib.rs` and `slang-sys/src/bindings.rs`.
  **Always keep this fork's version.** PR #35 gates bindings behind a `bindgen`
  feature and still generates in `build.rs`; this fork removed the feature
  entirely, moved bindgen to `xtask`, and proves the committed file on three
  platforms in CI. Upstream's design is a strict subset.
- **PR #34** (`search_paths`) — see `plans/02_search_paths_soundness.md`, which
  deliberately adopts the same public signature to keep this conflict small.

**Decision:** no action until a sync actually conflicts; this section is the
resolution note for whoever hits it.

## Verification

This document produces decisions, not code, so "done" means:

- §1 has a reproduction attempt with a recorded result — the only section with
  real investigation work in it, and the only one that can change what the
  library does.
- §2 has a yes/no from an actual consumer need, not a guess.
- §3 has a comment landed at `xtask/src/main.rs:124`.
- §4 has "state the bundled Slang version" folded into the release ritual in
  `plans/00_static_build.md`.
- Each section's decision is written back into the table at the top of this file.

## Risks

**§1 is open-ended.** If issue #26 still reproduces and Slang has not fixed it,
the wrapper-side fix touches the refcounting that every type in `src/lib.rs`
depends on — the highest-risk change contemplated across all three of these
plans. Time-box the investigation; do not start the fix in the same session that
discovers the answer.

**Deferring §2 has a cost if it is deferred forever.** PR #32 will bit-rot
against upstream's `SessionDesc` and against this fork's, and its author is not
going to rebase it indefinitely. If the answer to "do we want this" is *probably
eventually*, adopting sooner is cheaper than adopting later.
