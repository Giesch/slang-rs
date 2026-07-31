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

**Archives are vendored into the repo, not downloaded at build time.** Reproducible,
offline, network-free builds. To keep the blobs out of `main`'s history they live
only on release branches — see "Where the archives live" below.

## Where things stand

The `static` feature today (`slang-sys/build.rs:44-70`) requires `SLANG_EXTERNAL_DIR`
pointing at a Slang **source build tree**, and hardcodes three paths into it:

    miniz/Release/        lz4/build/cmake/Release/        cmark/src/Release/

Those `Release/` segments are MSVC multi-config generator layout. Under Ninja or
Make — our Linux and macOS builds — the libraries land one directory up. **The
current static path only works against a Windows/MSVC source build.**

Already in place and not part of this work:

- `link-cplusplus` is an optional dependency enabled by `static`, so the C++
  runtime (`stdc++` / `c++`) is handled.
- The `slang` → `slang-compiler` rename that blocks upstream #25 was fixed here in
  40be816.

Missing: `links = "slang"`, `-DSLANG_STATIC` in the bindgen clang args, and any
notion of an archive.

## What the archives actually contain

Confirmed against `release-static.yml`. This differs from what a source build
produces and drove several corrections to this plan.

`SLANG_BUNDLE_STATIC_LIB=ON` merges everything into **one** archive per platform:
`libslang-static.a` on Unix, `slang-static.lib` on Windows. The staging step ships
that single file and deliberately excludes `libslang-compiler.a`, which is
installed alongside but cannot be linked on its own. So the packaged `lib/`
contains exactly one library — not the six that the current `build.rs` links
individually.

Three platforms ship, named by the workflow's own labels rather than Rust triples:

| workflow platform | Rust target triple | notes |
| --- | --- | --- |
| `linux-x86_64` | `x86_64-unknown-linux-gnu` | built in `manylinux_2_28` for the glibc floor |
| `macos-aarch64` | `aarch64-apple-darwin` | native arm64, `CMAKE_OSX_DEPLOYMENT_TARGET=13.0` |
| `windows-x86_64` | `x86_64-pc-windows-msvc` | `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL` |

Packaged as `slang-static-<version>-<platform>.tar.gz` and `.zip`, each expanding
to `lib/`, `include/`, `licenses/`.

**`slang-glslang` is not dynamically loaded.** The build sets
`SLANG_EMBED_SLANG_GLSLANG=ON` with `SLANG_ENABLE_SLANG_GLSLANG=OFF`, and a
dedicated workflow step runs `ldd` / `otool -L` / `dumpbin //dependents` against
`slangc`, failing if `slang-glslang` or `slang-compiler` appears. The smoke test
covers `-O3`, `spirv-asm` and SPIR-V validation specifically because `-O0` emits
SPIR-V natively and would not exercise the embedded wrapper. The concern raised on
upstream #25 (shader-slang/slang#10652) is therefore already closed for us, by
construction and with a regression check.

The workflow's own consumer link check also pins down what a consumer must pass:
`-DSLANG_STATIC`, plus `-lstdc++ -lm -lpthread -ldl` on Linux and `-lc++ -lm
-lpthread` on macOS.

## Where the archives live

Keeping ~40 MB of already-compressed blobs out of `main` while still shipping them
through a plain Cargo git dependency:

- `main` holds source only, no archives, ever.
- Each release branches `release/vX` from a `main` commit, adds
  `slang-sys/vendor/`, commits, and tags. **The release commit is never merged
  back into `main`.**
- Consumers pin `tag = "vX"` rather than `branch = "main"`.

This works because Cargo fetches a git dependency with a targeted refspec derived
from the manifest reference — `refs/heads/<branch>` for `branch`, `refs/tags/<tag>`
for `tag` — rather than mirroring the repository. Archive blobs are reachable only
from release tags, so a `branch = "main"` consumer never receives them and a
`tag = "vX"` consumer receives exactly one release's set.

Caveats:

- **Pin by `tag`, not `rev`.** Cargo can fall back to a full fetch when a bare rev
  is not already present locally.
- `main` alone can no longer build `--features static`. Keep an environment escape
  hatch (`SLANG_STATIC_ARCHIVE_DIR`, pointing at an unpacked or packed archive) so
  `main` stays developable and CI can exercise the static path and regenerate
  bindings without cutting a release.
- A human `git clone` still pulls all tags; `--single-branch --no-tags` for a small
  working clone.
- Server-side repository size still grows per release. Only per-consumer transfer
  is bounded.
- **Verify empirically before committing to this.** Once the first release exists,
  pin the renderer at `branch = "main"` and measure `~/.cargo/git/db/`.

## Plan

### 1. Adopt the `c_char` fix (do this first, independent of everything else)

`src/lib.rs:673` and `src/lib.rs:731` use `*const i8` directly:

```rust
pub fn search_paths(mut self, paths: &'a [*const i8]) -> Self
fn push_strings(mut self, name: CompilerOptionName, s0: *const i8, s1: *const i8) -> Self
```

On aarch64, `CStr::as_ptr()` yields `*const u8`, so these signatures fail to
compile for callers. Not hypothetical, and not optional: the release workflow ships
a `macos-aarch64` archive, and `vulkan-slang-renderer` calls
`.search_paths(&[search_path.as_ptr()])` at `src/shaders.rs:70`, `:151` and `:227`.
Without this fix the macOS archive is unusable — and the failure lands in the
renderer, not here.

Switch both to `c_char`, matching upstream
[#37](https://github.com/FloatyMonkey/slang-rs/pull/37). On x86_64 `c_char` is
`i8`, so this is a no-op for existing consumers and needs no renderer change.

Land this as its own commit ahead of the archive work — it is small, orthogonal,
and unblocks arm64 testing of everything that follows.

### 2. Vendor the archives

Add `slang-sys/vendor/` on release branches only, holding one archive per platform:

    vendor/slang-static-<version>-linux-x86_64.tar.gz
    vendor/slang-static-<version>-macos-aarch64.tar.gz
    vendor/slang-static-<version>-windows-x86_64.tar.gz

Vendor the `.tar.gz` exactly as published, so each blob's SHA-256 can be checked
against the GitHub release asset. Recompressing to xz would save roughly 40% —
PR #3 measured the Linux archive at 21.6 MB with `gzip -9` against 13.4 MB with
`xz -9`, and the workflow packages at gzip's default level, so the real gap is
wider — but it breaks that provenance check. Start with the published `.tar.gz`;
revisit if the Windows size investigation leaves us needing the space.

Record the source release tag and each SHA-256 in a committed manifest. Plain git
blobs only — Cargo git dependencies do not resolve LFS pointers.

### 3. Rewrite the `static` path in `build.rs`

Replace the `SLANG_EXTERNAL_DIR` branch with:

1. Map `CARGO_CFG_TARGET_*` to a workflow platform name via the table above; fail
   with a clear message naming any unsupported triple.
2. Extract to `OUT_DIR` if not already present, verifying the manifest hash.
   Honour `SLANG_STATIC_ARCHIVE_DIR` as an override first.
3. Emit one `rustc-link-search=native={OUT_DIR}/.../lib`.
4. Emit **one** `rustc-link-lib=static=slang-static`. Delete the six existing
   `slang-compiler` / `compiler-core` / `core` / `miniz` / `lz4` / `cmark-gfm`
   lines — those components are already merged into the bundled archive, and
   nothing else is shipped for them to resolve against.
5. Emit the system libraries the workflow's consumer check proves are needed:
   `m`, `pthread`, `dl` on Linux; `m`, `pthread` on macOS. `link-cplusplus`
   continues to supply the C++ runtime.
6. Point the bindgen header at the extracted `include/`, so a static build needs
   no `SLANG_DIR` / `SLANG_INCLUDE_DIR` / `VULKAN_SDK` at all.

Net effect: four search paths become one, six link libraries become one, all
`Release/` guesswork disappears, and the static build stops depending on any
environment variable. The dynamic path keeps its current behaviour untouched.

### 4. Add `-DSLANG_STATIC` to the bindgen clang args

The archives are compiled with `SLANG_STATIC` and the workflow's own consumer link
check passes `-DSLANG_STATIC`; the headers must be parsed the same way here, or
`SLANG_API` resolves to `__declspec(dllimport)` on MSVC while the library exports
plain symbols.

### 5. Make `static` additive; add `links = "slang"`

Drop the `compile_error!` pair at `build.rs:5-8`. Cargo feature unification is
additive, so a graph where anything enables default features currently turns into
a hard build failure. Replace with: `static` wins when both are enabled, `dynamic`
remains the default when neither is specified.

Add `links = "slang"` to `slang-sys/Cargo.toml` so two copies in one graph produce
a comprehensible Cargo error rather than duplicate symbols at link time.

### 6. Check in generated bindings, regenerate under CI

Commit `slang-sys/src/bindings.rs` and put bindgen behind an opt-in feature, with a
CI job that regenerates from the archive's headers (via `SLANG_STATIC_ARCHIVE_DIR`
on `main`) and fails on diff.

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

Document the vendored-archive model and the tag-pinning requirement, drop
`SLANG_EXTERNAL_DIR`, and state which target triples ship an archive.

## Renderer follow-on

Switch the `shader-slang` dependency from `branch = "main"` to `tag = "..."`, then
`cargo check --all-targets`, `just shaders`, `just test`, and
`timeout 3 just dev EXAMPLE` for validation errors. No `+crt-static` is needed: the
archives use `MultiThreadedDLL`, matching Rust's `*-pc-windows-msvc` default.

Constraint: `import slang.neural` and `import experimental.workgraph` will not
resolve — the workflow drops those `.slang-module` files deliberately, since they
are loaded from disk relative to the host binary and a static link cannot absorb
them. Grep `shaders/source/` before switching.

## Prerequisites

Both must be resolved before step 2, since they determine what we vendor:

- **A static release must be tagged in `Giesch/slang`.**
  [PR #3](https://github.com/Giesch/slang/pull/3) merged to `master` on
  2026-07-31, so `release-static.yml` is now live there and a matching tag push
  will fire it. Nothing has been published yet — the four existing releases
  (`v2026.13.1`, `v2026.13`, `v2026.1.1`, `v2026.1-static`) all predate the merge,
  and the publish step has been skipped on every run so far.

  Tag naming needs care. The workflow derives archive names from
  `${GITHUB_REF_NAME#v}`, and the upstream-inherited `v2026.13` / `v2026.13.1`
  tags already exist, so the static release needs a distinct one. Follow the
  repository's own prior art and suffix it: `v2026.13.1-static` matches the
  trigger pattern and mirrors the existing `v2026.1-static`. A first pass tagged
  `v2026.13.1-static-draft` publishes as draft + prerelease, which exercises the
  never-yet-run publish path without committing to a public release.

- **The Windows archive size anomaly must be understood.** 141 MB versus 44 MB on
  Linux suggests debug records are still embedded per-object despite
  `SLANG_ENABLE_RELEASE_DEBUG_INFO=OFF`. A blocker for vendoring specifically,
  because it lands in git history permanently. Reconcile against PR #3's measured
  Linux baseline while investigating: 76.6 MB stripped, 21.6 MB at `gzip -9`.

## Risks

**Repository growth.** `.tar.gz` archives are already-compressed blobs, so git
cannot delta them; every Slang version bump adds the full set permanently. The
release-branch scheme bounds *per-consumer* transfer to one release's worth, but
not the repository's own size. An earlier ~13 MB-per-archive estimate predates the
141 MB Windows measurement and should be re-derived, not trusted. Escape hatch if
it becomes painful: periodic history squash of old release branches.

**Release ritual.** Cutting a release is now a branch-plus-tag operation rather
than a push to `main`, and fixes wanted in a release have to be cherry-picked onto
a fresh release branch. Acceptable for a repository that changes this rarely.
