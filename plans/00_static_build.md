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

**We vendor `.tar.xz`, not `.tar.gz`.** These blobs are committed permanently, so
compression ratio is the one cost that never goes away. `Giesch/slang` now
publishes a `.tar.xz` per platform alongside the `.tar.gz` and `.zip`, which cut
a vendored set from 114.7 MB to 62.3 MB. Vendoring a locally recompressed archive
is explicitly rejected: it would break the SHA-256-against-published-asset check
that makes the manifest worth having.

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

Packaged as `slang-static-<version>-<platform>` in `.tar.gz`, `.tar.xz` and
`.zip`, each expanding to `lib/`, `include/`, `licenses/`. We vendor the
`.tar.xz`.

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

Keeping a per-release set of compressed blobs — 62.3 MB as `.tar.xz` — out of
`main` while still shipping them through a plain Cargo git dependency:

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

Add `slang-sys/vendor/` on release branches only, holding one archive per platform.
For the current release that is:

    vendor/slang-static-2026.13.1-linux-x86_64.tar.xz
    vendor/slang-static-2026.13.1-macos-aarch64.tar.xz
    vendor/slang-static-2026.13.1-windows-x86_64.tar.xz

Unblocked: `v2026.13.1-static` ships `.tar.xz` per platform, 62.3 MB for the set.

Vendor the archives **exactly as published**, so each blob's SHA-256 can be
checked against the release asset — the published `SHA256SUMS` covers all nine
assets, and the three `.tar.xz` hashes are recorded under "Upstream release".
Never recompress locally: it saves nothing now and destroys that check, which is
the whole reason the manifest is worth having.

Record the source release tag (`v2026.13.1-static`) and each SHA-256 in a
committed manifest. Plain git blobs only — Cargo git dependencies do not resolve
LFS pointers.

`build.rs` will need an xz decoder — `xz2` or `liblzma`, in place of `flate2`
(see step 7's note on doing extraction in Rust rather than shelling out to
`tar`).

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

### 7. CI across all three platforms

The crate's static path is platform-specific in ways nothing else in this plan
tests: archive selection, extraction, link flags, and bindgen's target-dependent
output. It needs its own matrix, mirroring the platforms `release-static.yml`
ships:

| runner | target triple |
| --- | --- |
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` |
| `macos-latest` | `aarch64-apple-darwin` |
| `windows-latest` | `x86_64-pc-windows-msvc` |

Three jobs per platform:

1. **Build and test the static feature** — `cargo test --no-default-features
   --features static`. On `main` this uses `SLANG_STATIC_ARCHIVE_DIR`, pointed at
   an archive downloaded from the pinned `Giesch/slang` release. On release
   branches it must run against the *vendored* archive with no download, since
   that is the configuration consumers actually get.

2. **Bindings diff check** — regenerate with bindgen from the archive's headers
   and fail on any diff. **Run this on all three platforms, not just Linux.**
   bindgen output is target-dependent: `long` is 32-bit on MSVC and 64-bit
   elsewhere, MSVC struct layout differs, and `c_char` signedness differs on
   aarch64. A single committed `bindings.rs` may therefore not be valid
   everywhere. This job is what tells us whether one file suffices or whether
   step 6 needs `#[cfg]`-selected per-target bindings — decide from its result
   rather than assuming, since assuming is what makes the rustified-enum hazard
   in upstream #35 real rather than theoretical.

3. **Consumer smoke test** — an example that creates a global session and
   compiles a shader to SPIR-V at `-O3` through the Rust API. The slang
   workflow's own consumer check proves the archive links from C++; this proves
   it links and *runs* from Rust. `-O0` is not sufficient: it emits SPIR-V
   natively and never reaches the embedded glslang wrapper.

Platform notes:

- **Windows** is where `-DSLANG_STATIC` (step 4) fails if it is missing, and the
  job must *not* set `+crt-static` — the archive is built with `MultiThreadedDLL`
  to match Rust's default, and forcing the static CRT reintroduces the LNK2038
  the slang-side build was configured to avoid.
- **macOS** runners are arm64, so this job does not compile at all without the
  `c_char` fix from step 1. Set `MACOSX_DEPLOYMENT_TARGET=13.0` to match the
  archive.
- **Linux** needs a runner with glibc ≥ 2.28, the floor set by building in
  `manylinux_2_28`. `ubuntu-22.04` satisfies this.

Extraction in `build.rs` must be done with Rust crates (`xz2`, `tar`, `sha2`)
rather than by shelling out to `tar`, whose availability and flag handling differ
on Windows runners. That adds build-dependencies, which is a real cost — but a
smaller one than bindgen and libclang, which step 6 removes.

**Gate the release on this matrix.** A release branch must be green on all three
platforms before its tag is pushed. The slang-side workflow already proves the
archive is good; this proves the crate consuming it is good, and it is the last
check before consumers pin the tag.

### 8. Update the README

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

## Upstream release — available

**`Giesch/slang` `v2026.13.1-static` is published**, re-cut after the xz change,
with nine assets: `.tar.gz`, `.tar.xz` and `.zip` per platform, plus `SHA256SUMS`.
Both prerequisites are now cleared and step 2 can proceed.

Verified directly against the published archives:

- All three `.tar.xz` were downloaded and checked against the published
  `SHA256SUMS`: **OK**.
- `lib/` contains **exactly one file** on every platform — no
  `libslang-compiler.a`. This is the invariant step 3 depends on, now confirmed
  from the shipped artifact on all three rather than inferred from the workflow.
- The consumer link check passed on all three platforms against the packaged tree
  alone.

| platform | `.tar.gz` | `.tar.xz` | ratio | `lib/` contents, uncompressed |
| --- | --- | --- | --- | --- |
| macos-aarch64 | 19.8 MB | **11.8 MB** | 0.59 | `libslang-static.a` — 61,266,912 |
| linux-x86_64 | 22.8 MB | **13.8 MB** | 0.61 | `libslang-static.a` — 79,440,832 |
| windows-x86_64 | 72.0 MB | **36.7 MB** | 0.51 | `slang-static.lib` — 487,825,608 |
| **total** | **114.7 MB** | **62.3 MB** | 0.54 | |

xz beat the projection: 62.3 MB against the ~71 MB this plan estimated, a 52.3 MB
saving over `.tar.gz`. Windows outperformed the 0.62 ratio the others hold to,
landing at 0.51 — consistent with the guess that its bulk is highly compressible
debug records, which xz exploits more than gzip.

SHA-256 of the three `.tar.xz`, for the step 2 manifest:

```
e9bac199cb346ff832ed83b2cb5ef37b3613ff8644011af6da9fae796325a1b2  slang-static-2026.13.1-linux-x86_64.tar.xz
2a295356916a72ceba8a292849f03df41d6458c378e562922851397867e1559f  slang-static-2026.13.1-macos-aarch64.tar.xz
a52c07d29d3b5885a17c61292347bd7ffd022d8f67f6cd5848069bb36216d761  slang-static-2026.13.1-windows-x86_64.tar.xz
```

These supersede the hashes recorded before the re-cut. Moving the tag rebuilt
every asset, so the `.tar.gz` hashes changed too; nothing had been vendored yet,
so nothing needs re-verifying.

## Windows archive size — open, no longer blocking

Compression solved the vendoring cost, not the underlying problem. Windows ships
a **487.8 MB** `slang-static.lib` against 79.4 MB on Linux and 61.3 MB on macOS
for the same merged content — roughly 6× Linux, a far starker gap than the
compressed sizes suggested. That it then compresses *better* than the other
platforms (0.51 against 0.59-0.61) is itself the evidence: the excess is
redundant, highly compressible data, which points squarely at debug records
surviving `SLANG_ENABLE_RELEASE_DEBUG_INFO=OFF`.

Diagnose with `dumpbin /headers` over a few members of `slang-static.lib`,
grepping for `.debug$S` / `.debug$T`. If present,
`-DCMAKE_MSVC_DEBUG_INFORMATION_FORMAT=""` is the lever.

This no longer blocks step 2 — 36.7 MB vendored is acceptable — but it is not
purely cosmetic either, because the **extracted** size is what a Windows consumer
pays. `build.rs` unpacks the archive into `OUT_DIR`, so every Windows build of
this crate writes ~488 MB to disk and keeps it in the target directory. That is
the cost worth quoting when deciding whether to chase this.

## Risks

**Repository growth.** Compressed archives are opaque blobs, so git
cannot delta them; every Slang version bump adds the full set permanently. Exact,
from the published `v2026.13.1-static` `.tar.xz` assets: 13,844,500 + 11,780,780 +
36,724,456 = **62.3 MB per release**. The release-branch scheme bounds
*per-consumer* transfer to one release's worth, so a tag-pinned consumer still
fetches only its own platform archive — but the repository itself grows by
62.3 MB per bump. Escape hatch if that becomes painful: periodic history squash of
old release branches.

Windows is 59% of that total and remains the one reducible part, but at 36.7 MB it
is no longer the deciding factor — see "Windows archive size".

**Release ritual.** Cutting a release is now a branch-plus-tag operation rather
than a push to `main`, and fixes wanted in a release have to be cherry-picked onto
a fresh release branch. Acceptable for a repository that changes this rarely.
