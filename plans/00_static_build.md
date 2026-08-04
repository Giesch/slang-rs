# 00 — Static Slang via vendored prebuilt libraries

Status: implemented on this branch (steps 1–8); first release not yet cut

Note: kept as a record of how the static build was arrived at. The
`dynamic`/`static`/`regenerate-bindings` features it describes have since been
removed — the vendored static build is now the only build, and bindings are
committed without a way to regenerate them from this repo.
Scope: `Giesch/slang-rs` (this repo), with a follow-on bump in `Giesch/vulkan-slang-renderer`

## Goal

Make `features = ["static"]` work without a local Slang source tree, on every
platform we care about, by vendoring the prebuilt static libraries produced by
`Giesch/slang`'s `release-static.yml` workflow — as the published `.tar.xz`
archives, committed only on release tags — and expanding them from `build.rs`.

## Decisions taken

**This fork is permanent.** We are not staging work for an upstream PR. Upstream
[#25](https://github.com/FloatyMonkey/slang-rs/pull/25) has been open since Sep 2025
and the maintainer explicitly rejected adding dependencies to solve C++ runtime
linking — but this repo already depends on `link-cplusplus` for exactly that. That
divergence is deliberate and we keep it. Freed from upstream's constraints we can
also drop the mutually-exclusive feature design and check in generated bindings,
both of which upstream is still litigating.

**The static libs are vendored into the repo, not downloaded at build time.**
Reproducible, offline, network-free builds for consumers. To keep the blobs out
of `main`'s history entirely they exist only in release-tag commits — see
"Where the static libs live" below. `main` instead carries a `justfile` whose
fetch recipe materializes the same content into a gitignored directory, so
development builds and CI work with nothing binary committed.

**We vendor the published `.tar.xz` archives, not extracted trees.** This
decision flipped twice, and the second flip is forced. An intermediate
revision committed the extracted `lib/` + `include/` + `licenses/` trees to
avoid `build.rs` re-materializing ~488 MB into `OUT_DIR` per Windows project
and profile — and died on first contact: GitHub hard-rejects any file over
100 MB, and the extracted `slang-static.lib` is 465 MiB. LFS cannot rescue it
because Cargo git dependencies do not resolve LFS pointers. The archives
themselves are fine — 36.7 MB at worst, under even GitHub's 50 MB warning
threshold — so the archives are what release tags carry, exactly as published.

The consequences: `build.rs` regains an extraction step (pure-Rust `lzma-rs` +
`tar` + `sha2` build-dependencies, compiled only for the `static` feature) and
tag consumers pay the `OUT_DIR` expansion — once per profile, guarded by a
marker file. Development builds on `main` are unaffected: `just fetch-static`
extracts into `vendor-local/` and `build.rs` links those trees in place with
no extraction. The SHA-256-against-published-asset check now runs twice: the
justfile verifies downloads at fetch time, and `build.rs` verifies the
vendored archive against the committed manifest before every extraction, so
the provenance chain survives into consumer builds.

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
notion of a vendored release.

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
`.zip`, each expanding to `lib/`, `include/`, `licenses/`. The fetch recipe
downloads the `.tar.xz` — smallest asset, and the pinned hashes below are for
it — and release tags commit those archives exactly as published.

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

## Where the static libs live

Keeping 62.3 MB of per-release archives out of `main` while still shipping
them through a plain Cargo git dependency:

- `main` holds source plus the `justfile`, no binaries, ever.
- `just fetch-static` downloads the pinned release's `.tar.xz` set from
  `Giesch/slang`, verifies each against the SHA-256 hashes committed in this
  repo, and extracts into `slang-sys/vendor-local/<platform>/`, which is
  gitignored. This is how `main` development and CI get a working
  `--features static` build: one command, no environment variables.
- `just release <tag>` re-runs the fetch, copies the `.tar.xz` archives to
  `slang-sys/vendor/` (not gitignored), commits, tags, and pushes **only the
  tag** — never `main`. The local branch is then reset, so the release commit
  has a `main` commit as parent but is reachable only from its tag. **There are
  no release branches.**
- Consumers pin `tag = "vX"` rather than `branch = "main"`; their `build.rs`
  verifies and expands the vendored archive into `OUT_DIR` on first build.

This works because Cargo fetches a git dependency with a targeted refspec derived
from the manifest reference — `refs/heads/<branch>` for `branch`, `refs/tags/<tag>`
for `tag` — rather than mirroring the repository. The binary blobs are reachable
only from release tags, so a `branch = "main"` consumer never receives them and a
`tag = "vX"` consumer receives exactly one release's set.

Caveats:

- **Pin by `tag`, not `rev`.** Cargo can fall back to a full fetch when a bare rev
  is not already present locally.
- A fresh clone of `main` cannot build `--features static` until
  `just fetch-static` has run. That recipe *is* the escape hatch — the
  `SLANG_STATIC_ARCHIVE_DIR` environment variable from earlier revisions of
  this plan is dropped; CI runs the same recipe a developer does.
- A human `git clone` still pulls all tags, and with them every release's
  archives — 62.3 MB per tag. Use `--single-branch --no-tags` for a small
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

Land this as its own commit ahead of the vendoring work — it is small, orthogonal,
and unblocks arm64 testing of everything that follows.

### 2. Add the justfile; vendor the archives on release tags only

Two recipes at the repo root, plus a committed manifest pinning the source
release tag (`v2026.13.1-static`) and the three `.tar.xz` SHA-256 hashes
recorded under "Upstream release".

**`just fetch-static`** — how `main` development and CI get the libs:

1. Download `slang-static-2026.13.1-<platform>.tar.xz` for all three platforms
   from the pinned `Giesch/slang` release.
2. Verify each against the pinned SHA-256; fail loudly on mismatch. The hashes
   pin the exact published assets, not whatever the release tag points at
   later.
3. Extract each into `slang-sys/vendor-local/<platform>/` (`lib/`, `include/`,
   `licenses/`). Add `vendor-local/` to `.gitignore` in the same commit that
   adds the justfile.

GitHub's runners all handle this, including Windows, whose system `tar` is
bsdtar with xz support — the recipe can shell out to `curl`/`tar` because it
runs for maintainers and CI, never for consumers.

**`just release <tag>`** — cuts a release:

1. Require a clean working tree on an up-to-date `main`.
2. Run `just fetch-static` (re-verifies the hashes).
3. Copy the three `.tar.xz` from `vendor-local/` into `slang-sys/vendor/`.
4. Commit, tag `<tag>`, push only the tag, and reset the local branch — the
   release commit never lands on `main`.

The vendored archives are plain git blobs — Cargo git dependencies do not
resolve LFS pointers, so LFS stays off the table (and nothing vendored may
exceed GitHub's 100 MB hard limit, which is what killed the extracted-trees
revision of this plan). Provenance stays checkable forever: each vendored
archive is byte-identical to the published release asset and matches the
committed manifest, which `build.rs` re-verifies before every extraction.

### 3. Rewrite the `static` path in `build.rs`

Replace the `SLANG_EXTERNAL_DIR` branch with:

1. Map `CARGO_CFG_TARGET_*` to a workflow platform name via the table above; fail
   with a clear message naming any unsupported triple.
2. Locate the platform tree: if `slang-sys/vendor/` holds this platform's
   `.tar.xz` (release tags), verify it against the committed manifest and
   expand it into `OUT_DIR` — once per profile, behind a marker file; else use
   `slang-sys/vendor-local/<platform>/` (populated by `just fetch-static`),
   linked in place with no extraction; else fail with a message saying to run
   `just fetch-static` on `main` or depend on a release tag.
3. Emit one `rustc-link-search=native=<tree>/lib`.
4. Emit **one** `rustc-link-lib=static=slang-static`. Delete the six existing
   `slang-compiler` / `compiler-core` / `core` / `miniz` / `lz4` / `cmark-gfm`
   lines — those components are already merged into the bundled archive, and
   nothing else is shipped for them to resolve against.
5. Emit the system libraries the workflow's consumer check proves are needed:
   `m`, `pthread`, `dl` on Linux; `m`, `pthread` on macOS. `link-cplusplus`
   continues to supply the C++ runtime.
6. Point the bindgen header at `<tree>/include`, so a static build needs
   no `SLANG_DIR` / `SLANG_INCLUDE_DIR` / `VULKAN_SDK` at all.

Extraction is done with pure-Rust crates (`lzma-rs`, `tar`, `sha2`) rather
than by shelling out to `tar`, whose availability and flag handling differ on
Windows; they are optional build-dependencies compiled only for the `static`
feature. Net effect: four search paths become one, six link libraries become
one, all `Release/` guesswork disappears, and the static build stops depending
on any environment variable. The dynamic path keeps its current behaviour
untouched.

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
CI job that regenerates from the vendored headers (on `main`, from the tree
`just fetch-static` leaves in `vendor-local/`) and fails on diff.

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
tests: platform-tree selection, link flags, and bindgen's target-dependent
output. It needs its own matrix, mirroring the platforms `release-static.yml`
ships:

| runner | target triple |
| --- | --- |
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` |
| `macos-latest` | `aarch64-apple-darwin` |
| `windows-latest` | `x86_64-pc-windows-msvc` |

Three jobs per platform:

1. **Build and test the static feature** — `cargo test --no-default-features
   --features static`. On `main` the job runs `just fetch-static` first. On a
   release tag it must run against the *vendored* tree with no download and no
   fetch recipe, since that is the configuration consumers actually get.

2. **Bindings diff check** — regenerate with bindgen from the vendored (or
   fetched) `include/` and fail on any diff. **Run this on all three platforms, not just Linux.**
   bindgen output is target-dependent: `long` is 32-bit on MSVC and 64-bit
   elsewhere, MSVC struct layout differs, and `c_char` signedness differs on
   aarch64. A single committed `bindings.rs` may therefore not be valid
   everywhere. This job is what tells us whether one file suffices or whether
   step 6 needs `#[cfg]`-selected per-target bindings — decide from its result
   rather than assuming, since assuming is what makes the rustified-enum hazard
   in upstream #35 real rather than theoretical.

   *Outcome, from the first real runs:* one file suffices. The only variance
   was platform-introspection macros, the `#[link_name]` mangling of the two
   functions slang.h declares without `extern "C"` (both unused here — now
   blocklisted), and MSVC giving `int` to five unscoped enums where Itanium
   picks `unsigned int` (normalized to `u32` after generation; both 32-bit).

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

`build.rs` extracts with Rust crates, so nothing at consumer build time
depends on `tar` or `xz` binaries. The fetch recipe does shell out to them,
but it runs on maintainer machines and CI runners, all of which have bsdtar.
The runners need `just` installed (one setup-action line). Note the coverage
split: off-tag runs build from `vendor-local/` trees and never exercise the
archive-extraction path in `build.rs` — only tag runs do. That path is what
the tag-gated matrix exists to prove before consumers pin the tag.

**Gate the tag on this matrix.** There is no release branch to gate, so the
matrix runs on tag push, against the vendored tree. A red run means delete the
tag, fix on `main`, and re-cut — cheap, provided nothing is announced or pinned
against the tag until the run is green. Never repoint a tag a consumer may
already have resolved. The slang-side workflow already proves the archive is
good; this proves the crate consuming it is good, and it is the last check
before consumers pin the tag.

### 8. Update the README

Document the vendoring model — `just fetch-static` on `main`, tag-pinning for
consumers — drop `SLANG_EXTERNAL_DIR`, and state which target triples ship a
static lib.

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

SHA-256 of the three `.tar.xz`, pinned in the repo for `just fetch-static`
verification:

```
e9bac199cb346ff832ed83b2cb5ef37b3613ff8644011af6da9fae796325a1b2  slang-static-2026.13.1-linux-x86_64.tar.xz
2a295356916a72ceba8a292849f03df41d6458c378e562922851397867e1559f  slang-static-2026.13.1-macos-aarch64.tar.xz
a52c07d29d3b5885a17c61292347bd7ffd022d8f67f6cd5848069bb36216d761  slang-static-2026.13.1-windows-x86_64.tar.xz
```

These supersede the hashes recorded before the re-cut. Moving the tag rebuilt
every asset, so the `.tar.gz` hashes changed too; nothing had been vendored yet,
so nothing needs re-verifying.

## Windows static lib size — open, no longer blocking

Compression solved the transfer cost, not the underlying problem. Windows ships
a **487.8 MB** `slang-static.lib` against 79.4 MB on Linux and 61.3 MB on macOS
for the same merged content — roughly 6× Linux, a far starker gap than the
compressed sizes suggested. That it then compresses *better* than the other
platforms (0.51 against 0.59-0.61) is itself the evidence: the excess is
redundant, highly compressible data, which points squarely at debug records
surviving `SLANG_ENABLE_RELEASE_DEBUG_INFO=OFF`.

Diagnose with `dumpbin /headers` over a few members of `slang-static.lib`,
grepping for `.debug$S` / `.debug$T`. If present,
`-DCMAKE_MSVC_DEBUG_INFORMATION_FORMAT=""` is the lever.

This anomaly briefly drove a switch to vendoring extracted trees — eliminating
the `OUT_DIR` expansion by committing the libraries directly — before GitHub's
100 MB hard file limit killed that revision (the 465 MiB `slang-static.lib`
can never be a git blob on GitHub). With archives vendored again, the
uncompressed cost lands where it originally did: every Windows build of a
release tag expands ~490 MB into `OUT_DIR`, once per project and profile.
Fixing the underlying bloat would shrink that, the 36.7 MB per-release repo
growth for Windows, and the transfer; worth chasing, not blocking.

## Risks

**Repository growth.** Compressed archives are opaque blobs git cannot delta,
so every Slang version bump adds a full set permanently: 13,844,500 +
11,780,780 + 36,724,456 = **62.3 MB per release**. The tag-only scheme bounds
*per-consumer* transfer to one release's set; a tag-pinned consumer's checkout
carries the 62.3 MB of archives plus, on first build, the `OUT_DIR` expansion
of its own platform's archive (~490 MB on Windows, ~80 MB elsewhere, once per
profile). Escape hatches if growth becomes painful: delete release tags
nothing depends on anymore — the commits become unreachable and server GC
reclaims them — or revisit per-platform release tags.

Windows is 59% of the per-release total and remains the one reducible part —
see "Windows static lib size".

**Release ritual.** Cutting a release is `just release <tag>` — one command, no
release branches, no merge-back discipline. A fix wanted in a release is a
commit on `main` followed by a re-cut. The residual risk is tag hygiene: never
move or delete a tag consumers may have pinned.
