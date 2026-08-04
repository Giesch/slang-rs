# 01 — Bitfield-shaped enums (`SlangResourceShape`, `SlangBindingType`)

Status: proposed
Scope: `slang-sys` bindings (`xtask` bindgen config) + `shader-slang` wrapper
Upstream: [FloatyMonkey/slang-rs#28](https://github.com/FloatyMonkey/slang-rs/issues/28)

## Goal

Stop constructing invalid enum discriminants when Slang returns a resource shape
or binding type that `slang.h` does not spell out as a named enumerator, and give
the high-level crate an API that models these values as what they actually are:
a base shape plus flag bits.

## Where things stand

`slang.h` declares both of these as bitfields. `SlangResourceShape`
(`slang-sys/vendor-local/<platform>/include/slang.h:2106`) carries mask
enumerators and flag bits alongside its base shapes:

    SLANG_RESOURCE_BASE_SHAPE_MASK = 0x0F
    SLANG_TEXTURE_1D = 0x01 ... SLANG_TEXTURE_SUBPASS = 0x0A

    SLANG_RESOURCE_EXT_SHAPE_MASK  = 0x1F0
    SLANG_TEXTURE_FEEDBACK_FLAG    = 0x10
    SLANG_TEXTURE_SHADOW_FLAG      = 0x20
    SLANG_TEXTURE_ARRAY_FLAG       = 0x40
    SLANG_TEXTURE_MULTISAMPLE_FLAG = 0x80
    SLANG_TEXTURE_COMBINED_FLAG    = 0x100

then names exactly six of the 5 × 11 possible base|flag products
(`SLANG_TEXTURE_1D_ARRAY`, `2D_ARRAY`, `CUBE_ARRAY`, `2D_MULTISAMPLE`,
`2D_MULTISAMPLE_ARRAY`, `SUBPASS_MULTISAMPLE`). `SlangBindingType` (`slang.h:2278`)
has the same structure with one flag, `SLANG_BINDING_TYPE_MUTABLE_FLAG = 0x100`,
and three named products.

bindgen turns both into real Rust enums — `slang-sys/src/bindings.rs:1661` is
`#[repr(u32)] pub enum SlangResourceShape` with 24 variants, masks included as if
they were shapes. Every other bit pattern Slang can produce has **no matching
discriminant**. `SLANG_TEXTURE_2D | SLANG_TEXTURE_COMBINED_FLAG` (0x102) is the
case that surfaced the issue; `TEXTURE_CUBE | COMBINED_FLAG`, anything with
`SHADOW_FLAG`, and any `FEEDBACK_FLAG` combination are equally reachable.

These values arrive through `rcall!` (`src/reflection/mod.rs:31`), which is a bare
`unsafe { sys::$f(...) }` with the enum as the declared return type:

- `src/reflection/ty.rs:82` — `resource_shape()`
- `src/reflection/type_layout.rs:129` — `resource_shape()`
- `src/reflection/type_layout.rs:164` — `binding_range_type()`
- `src/reflection/type_layout.rs:274` — `descriptor_set_descriptor_range_type()`

Materializing an enum with an out-of-range discriminant is immediate UB, not a
merely-surprising value: the compiler is entitled to assume the value is one of
the 24, and `match` arms, niche layouts and `Option<ResourceShape>` all rely on
that. So this is a live memory-safety bug for any consumer reflecting a combined
texture-sampler, a shadow sampler, or a feedback texture.

Both types are re-exported publicly as `ResourceShape` and `BindingType`
(`src/lib.rs:15`, `:22`), so the shape of the fix is a public API decision.

**This fix is much cheaper here than upstream.** Since `3cf7d94` moved bindgen
into `xtask` and the headers are pinned to the exact libraries being linked, it
is a config change plus a regen with a CI diff check behind it. Upstream still
generates at build time against whatever Slang header happens to be installed,
which is why the same change there also has to answer HampusMat's version-skew
objection on [#35](https://github.com/FloatyMonkey/slang-rs/pull/35).

## Decisions taken

**`bitfield_enum`, not `newtype_enum`.** Both produce a `#[repr(transparent)]`
newtype that can hold any bit pattern, which is all that is needed to kill the
UB. `bitfield_enum` additionally emits `BitOr`/`BitOrAssign`/`BitAnd`/`BitAndAssign`,
which is what the wrapper's accessors are built out of, and it documents intent:
these are bitfields.

**No `bitflags` dependency.** `shader-slang` has exactly one non-optional
dependency (`shader-slang-sys`) and `slang-sys` has one (`link-cplusplus`). Two
flag sets with five and one flags do not justify changing that; hand-written
predicates are shorter than the derive would be.

**The wrapper gets its own types rather than re-exporting the sys newtype.** A
raw `pub struct ResourceShape(pub u32)` is a worse public API than what exists
today, and the orphan rule forbids adding inherent methods to a `slang-sys` type
from `shader-slang`. Wrapping is the only way to offer `base()` / `is_array()`.

**Breaking change, accepted.** Consumers that `match` on `ResourceShape` variants
must move to `base()` plus flag predicates. This fork is not published to
crates.io — the README pins consumers to git tags — so the blast radius is
`Giesch/vulkan-slang-renderer`, which is the code that found the bug.

**Not staged for upstream.** Consistent with `plans/00_static_build.md`: upstream
cannot make this change as cleanly while bindings are generated at build time.
Fix it here; if upstream later adopts something similar, reconcile then.

## Facts already verified — do not re-derive

- `ParseCallback::add_attributes` (`xtask/src/main.rs:195`) keeps firing for
  newtype enums. bindgen 0.72.1 calls it with a hardcoded
  `kind: DeriveTypeKind::Enum` for **every** non-const variation
  (`bindgen-0.72.1/codegen/mod.rs:3762`, inside `if !variation.is_const()`), so
  the `#[cfg_attr(feature = "serde", derive(...))]` line still lands on both
  types.
- `normalize_enum_reprs` (`xtask/src/main.rs:140`) is unaffected. It matches
  `#[repr(i32)]` followed by `pub enum`; both of these have a fixed
  `unsigned int` underlying type in the header, so they were already `#[repr(u32)]`
  and were never in `ITANIUM_U32_ENUMS`.
- `enum_variant_name` (`xtask/src/main.rs:172`) still applies — variants become
  associated consts keeping their current names (`Texture2d`, `MutableTeture`,
  upstream typo included). Non-upper-case const names are already permitted by
  `#![allow(non_upper_case_globals)]` at `slang-sys/src/lib.rs:4`.
- Nothing inside this repository `match`es on either type, so step 1 compiles on
  its own with no call-site changes.

## Plan

### 1. Switch both enums to bitfields in `slang-sys`

In `xtask/src/main.rs`, next to the existing `.constified_enum(...)` calls at
lines 122-123:

```rust
.bitfield_enum("SlangResourceShape")
.bitfield_enum("SlangBindingType")
```

Then `just fetch-static && just regen-bindings`. Expect `slang-sys/src/bindings.rs`
to replace the two `pub enum` blocks with `#[repr(transparent)] pub struct` +
`impl` blocks of associated consts + bit-op impls. `cargo test` should still pass
untouched — this step alone removes the UB.

Commit this separately from step 2. It is the safety fix; step 2 is ergonomics.

### 2. Add `src/resource_shape.rs` with the safe wrapper types

```rust
pub enum BaseShape {
    None, Texture1D, Texture2D, Texture3D, TextureCube, TextureBuffer,
    StructuredBuffer, ByteAddressBuffer, Unknown, AccelerationStructure,
    TextureSubpass,
    Unrecognized(u32),   // a base shape a future Slang adds — never UB again
}

pub struct ResourceShape(sys::SlangResourceShape);

impl ResourceShape {
    pub fn base(self) -> BaseShape;          // value & BASE_SHAPE_MASK
    pub fn is_feedback(self) -> bool;
    pub fn is_shadow(self) -> bool;
    pub fn is_array(self) -> bool;
    pub fn is_multisample(self) -> bool;
    pub fn is_combined(self) -> bool;
    pub fn raw(self) -> sys::SlangResourceShape;
}

pub enum BaseBindingType { /* the 15 non-flag binding types */, Unrecognized(u32) }

pub struct BindingType(sys::SlangBindingType);

impl BindingType {
    pub fn base(self) -> BaseBindingType;    // value & BASE_MASK
    pub fn is_mutable(self) -> bool;         // value & MUTABLE_FLAG
    pub fn raw(self) -> sys::SlangBindingType;
}
```

Derive `Debug, Clone, Copy, PartialEq, Eq, Hash` on all four. Write `Debug` by
hand for `ResourceShape`/`BindingType` so it prints `Texture2D|ARRAY` rather than
a bare integer — this is the type people will be staring at in reflection dumps.

The `Unrecognized(u32)` arm is the point of the exercise: it makes the decode
total, so a Slang release that adds `SLANG_TEXTURE_*` base shape 0x0B degrades to
a value the consumer can see and report, not UB.

### 3. Return the wrapper types from the reflection API

Change return types at `src/reflection/ty.rs:82` and `src/reflection/type_layout.rs:129`,
`:164`, `:274` to the new `ResourceShape` / `BindingType`, wrapping the `rcall!`
result. `resource_access()` (`ty.rs:86`, `type_layout.rs:133`) is **not** a
bitfield — `SlangResourceAccess` enumerates all its values including
`UNKNOWN = 0x7FFFFFFF` — so leave it alone.

Update the re-export list in `src/lib.rs:14-26`: drop
`SlangResourceShape as ResourceShape` and `SlangBindingType as BindingType`, and
export the new types from `resource_shape` instead. Keeping the public names
identical means consumers see a change in the type's *shape*, not a rename.

### 4. serde

A newtype serializes as a bare number where the old enum serialized as a variant
name, so `--features serde` output changes for anything containing a resource
shape or binding type. Decide one of:

- accept the numeric representation (simplest; it is stable and round-trips), or
- hand-write `Serialize`/`Deserialize` for the wrapper types under
  `#[cfg(feature = "serde")]` emitting `{"base": "Texture2D", "flags": ["array"]}`.

Prefer the first unless `Giesch/vulkan-slang-renderer` serializes reflection data
somewhere a human reads it.

### 5. Update the README

The README does not currently mention either type, so this is only warranted if
step 4 changes serde output — in which case note it wherever the `serde` feature
is described.

## Verification

- `cargo test` and `cargo test --features serde`.
- `just regen-bindings && git diff --exit-code slang-sys/src/bindings.rs` clean;
  CI's existing bindings-diff job (`.github/workflows/ci.yml`) then proves the
  newtypes are target-independent on Linux, macOS and Windows.
- **Extend the fixture.** `shaders/test.slang` is three structured buffers, none
  of which exercises a flag bit. Add a `Texture2DArray`, a `Sampler2DShadow` and
  a combined `Sampler2D` (which is what produces `TEXTURE_2D | COMBINED_FLAG`,
  the 0x102 in issue #28), then assert in `src/tests.rs` that `base()` and the
  predicates decompose each correctly. Note this changes the existing
  `assert_eq!(reflection.parameter_count(), 3)` at `src/tests.rs:39` — update it.
- Before/after check against the original repro: the asserts in
  `Giesch/vulkan-tutorial-ash-sdl` on the `resource-shape-example` branch,
  `src/shaders/reflection/parameters.rs:233-246`. Running the pre-fix build under
  `-Zsanitizer=address` / `cargo +nightly miri` is optional but is the clearest
  demonstration that this was UB and not a cosmetic problem.

## Risks

**Churn in `bindings.rs`.** Two enums become newtype structs, so the diff is
large and mechanical. It is fully regenerable and CI diffs it on three platforms,
so the risk is review noise rather than correctness — which is the argument for
landing step 1 as its own commit.

**Exhaustiveness is lost at the sys layer.** After step 1, `match` on
`sys::SlangResourceShape` no longer type-checks, and a consumer that never
upgrades to the step 2 wrapper gets a raw integer newtype. Steps 2-3 are
therefore not optional polish; land them in the same PR.

**`Unrecognized` hides real problems.** A base shape that starts showing up as
`Unrecognized(11)` means the pinned Slang gained a shape and the wrapper needs a
variant. That is the correct failure mode — visible and non-UB — but it should be
loud: consider `debug_assert!` or a `tracing`-free `eprintln!`-equivalent... or
simply make sure the `Debug` impl prints it unmistakably.
