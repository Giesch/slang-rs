# 00 — Static Slang via vendored prebuilt archives

Status: proposed
Scope: `Giesch/slang-rs` (this repo), with a follow-on bump in `Giesch/vulkan-slang-renderer`

## Goal

Make `features = ["static"]` work without a local Slang source tree, on every
platform we care about, by vendoring prebuilt static archives produced by
`Giesch/slang`'s `release-static.yml` workflow and expanding them in `build.rs`.

## Decisions taken

**This fork is permanent.** We are not staging work for an upstream PR. Upstream
[#25](https://github.com/FloatyMonkey/slang-rs/pull/25) has been open since Sep 2025
and the maintainer explicitly rejected adding dependencies to solve C++ runtime
linking — but this repo already depends on `link-cplusplus` for exactly that. That
divergence is deliberate and we keep it. Freed from upstream's constraints we can
also drop the mutually-exclusive feature design and check in generated bindings,
both of which upstream is still litigating.

**Archives are vendored into the repo, not downloaded at build time.** Cargo git
dependencies are not target-filtered, so every consumer clones all platforms'
archives regardless of what they build for. We accept that cost in exchange for
reproducible, offline, network-free builds. See "Repository growth" under Risks.

## Where things stand

The `static` feature today (`slang-sys/build.rs:44-70`) requires `SLANG_EXTERNAL_DIR`
pointing at a Slang **source build tree**, and hardcodes three paths into it:

    miniz/Release/        lz4/build/cmake/Release/        cmark/src/Release/

Those `Release/` segments are MSVC multi-config generator layout. Under Ninja or
Make — our Linux and macOS builds — the libraries land directly in `miniz/`,
`lz4/build/cmake/` and `cmark/src/` with no `Release/` component. **The current
static path only works against a Windows/MSVC source build.**

Already in place and not part of this work:

- `link-cplusplus` is an optional dependency enabled by `static`, so the C++
  runtime (`stdc++` / `c++`) is handled. `m`, `pthread` and `dl` arrive via Rust's
  own libc linkage on Linux.
- The `slang` → `slang-compiler` rename that blocks upstream #25 was fixed here in
  40be816.

Missing: `links = "slang"`, `-DSLANG_STATIC` in the bindgen clang args, and any
notion of an archive.

## Plan

### 1. Adopt the `c_char` fix (do this first, independent of everything else)

`src/lib.rs:673` and `src/lib.rs:731` use `*const i8` directly:

```rust
pub fn search_paths(mut self, paths: &'a [*const i8]) -> Self
fn push_strings(mut self, name: CompilerOptionName, s0: *const i8, s1: *const i8) -> Self
```

On aarch64, `CStr::as_ptr()` yields `*const u8`, so these signatures fail to
compile for callers. This is not hypothetical for us: `vulkan-slang-renderer`
calls `.search_paths(&[search_path.as_ptr()])` at `src/shaders.rs:70`, `:151` and
`:227`. Any arm64 target — a macOS archive, or aarch64 Linux — breaks at the
renderer, not just here.

Switch both to `c_char`, matching upstream
[#37](https://github.com/FloatyMonkey/slang-rs/pull/37). On x86_64 `c_char` is
`i8`, so this is a no-op for existing consumers and needs no renderer change.

Land this as its own commit ahead of the archive work — it is small, orthogonal,
and unblocks arm64 testing of everything that follows.

### 2. Vendor the archives

Add `slang-sys/vendor/` holding one xz-compressed archive per target triple, named
predictably, e.g.:

    vendor/slang-static-x86_64-unknown-linux-gnu.tar.xz
    vendor/slang-static-x86_64-pc-windows-msvc.tar.xz
    vendor/slang-static-aarch64-apple-darwin.tar.xz

Each expands to the layout `release-static.yml` already produces: `lib/`,
`include/`, `licenses/`.

Record the source release tag and each archive's SHA-256 in a committed manifest
so `build.rs` can verify what it extracted and so provenance is auditable. Plain
git blobs only — Cargo git dependencies do not resolve LFS pointers.

### 3. Rewrite the `static` path in `build.rs`

Replace the `SLANG_EXTERNAL_DIR` branch with:

1. Select the archive for `CARGO_CFG_TARGET_*`; fail with a clear message naming
   the unsupported triple.
2. Extract to `OUT_DIR` if not already present, verifying the manifest hash.
3. Emit one `rustc-link-search=native={OUT_DIR}/.../lib`.
4. Keep the existing six `rustc-link-lib=static=` lines unchanged:
   `slang-compiler`, `compiler-core`, `core`, `miniz`, `lz4`, `cmark-gfm`.
5. Point the bindgen header at the extracted `include/`, so a static build needs
   no `SLANG_DIR` / `SLANG_INCLUDE_DIR` / `VULKAN_SDK` at all.

Net effect: four search paths collapse to one, all `Release/` guesswork
disappears, and the static build stops depending on any environment variable.

The dynamic path keeps its current environment-variable behaviour untouched.

### 4. Add `-DSLANG_STATIC` to the bindgen clang args

The archives are compiled with `SLANG_STATIC`; the headers must be parsed the same
way, or `SLANG_API` resolves to `__declspec(dllimport)` on MSVC while the library
exports plain symbols. Verify empirically on Windows whether this manifests as a
link failure — bindgen's handling of `dllimport` makes the failure mode
non-obvious — but set it regardless for correctness.

### 5. Make `static` additive; add `links = "slang"`

Drop the `compile_error!` pair at `build.rs:5-8`. Cargo feature unification is
additive, so a graph where anything enables default features currently turns into
a hard build failure. Replace with: `static` wins when both are enabled, `dynamic`
remains the default when neither is specified.

Add `links = "slang"` to `slang-sys/Cargo.toml` so two copies in one graph produce
a comprehensible Cargo error rather than duplicate symbols at link time.

### 6. Check in generated bindings, regenerate under CI

Commit `slang-sys/src/bindings.rs` and put bindgen behind an opt-in feature, with a
CI job that regenerates from the vendored headers and fails on diff.

Upstream [#35](https://github.com/FloatyMonkey/slang-rs/pull/35) is stalled on three
maintainer objections. Vendoring answers the substantive one: version skew between
stale headers and a newer shared library is impossible when headers and library
ship in the same archive, committed together — which matters specifically because
the enums are rustified and would otherwise accept unknown variants from a newer
library. The other two objections (writing outside `OUT_DIR`, races on the shared
registry cache) are artifacts of *generating* bindings at build time and do not
apply once they are committed and regeneration is opt-in.

Payoff: the default build stops needing bindgen, libclang and a Slang header tree.

### 7. Update the README

Document the vendored-archive model, drop `SLANG_EXTERNAL_DIR`, and state which
target triples ship an archive.

## Renderer follow-on

Bump the `shader-slang` rev off `40be816`, then `cargo check --all-targets`,
`just shaders`, `just test`, and `timeout 3 just dev EXAMPLE` for validation
errors. The MSVC runtime is already compatible: the archives are built with
`CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL`, which matches Rust's
`*-pc-windows-msvc` default, so no consumer needs `+crt-static`.

Constraint: `import slang.neural` and `import experimental.workgraph` will not
resolve, because the `.slang-module` files are deliberately not shipped in the
archives. Grep `shaders/source/` before switching.

## Prerequisites

Both must be resolved before step 2, since they determine what we vendor:

- **`Giesch/slang` PR #3 must be merged and a release tagged.** No archive exists
  to vendor until the publish path has actually run — it has been skipped on every
  job so far because the PR is a draft.
- **The Windows archive size anomaly must be understood.** 141 MB versus 44 MB on
  Linux suggests debug records are still embedded per-object despite
  `SLANG_ENABLE_RELEASE_DEBUG_INFO=OFF`. This is a blocker for vendoring
  specifically, because it lands in git history permanently.

## Open questions

- Does `lib/` in the produced archives actually contain all six libraries we link?
  If the CMake install stage exported only a subset, that is a packaging fix in
  `Giesch/slang` before any of this works.
- Which target triples do we ship? Linux x86_64 is required. macOS is presumably
  arm64, which makes step 1 a hard prerequisite rather than a nicety.
- Slang `dlopen`s `slang-glslang` at runtime
  ([shader-slang/slang#10652](https://github.com/shader-slang/slang/issues/10652)),
  so "static" is not absolute. The renderer compiles `.slang` to SPIR-V and never
  takes the GLSL passthrough path, so this should be latent — but confirm what the
  build did with `SLANG_ENABLE_SLANG_GLSLANG` before describing the result as fully
  static anywhere user-facing.

## Risks

**Repository growth.** xz archives are already-compressed blobs, so git cannot
delta them; every Slang version bump adds the full set to history permanently, and
shallow clones do not help since Cargo fetches the tip. Sizing depends on the
Windows anomaly above — an earlier ~13 MB-per-archive estimate predates the 141 MB
Windows measurement and should be re-derived, not trusted. Escape hatches if it
becomes painful: an orphan branch holding only archives, or a periodic history
squash. Recommend accepting the cost now and re-evaluating at the third version
bump.

**Per-consumer clone cost.** Every consumer pays for all platforms. Acceptable
while `vulkan-slang-renderer` is the only consumer; revisit if that changes.
