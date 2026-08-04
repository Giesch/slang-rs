# 02 — `SessionDesc::search_paths` soundness

Status: implemented on this branch (steps 1–5)
Scope: `shader-slang` public API
Upstream: [FloatyMonkey/slang-rs#31](https://github.com/FloatyMonkey/slang-rs/issues/31),
with an open fix in [#34](https://github.com/FloatyMonkey/slang-rs/pull/34)

## Goal

Make `SessionDesc::search_paths` take owned-or-borrowed Rust strings and keep the
C strings alive for as long as Slang needs them, so that a safe caller can no
longer trigger undefined behaviour.

## Where things stand

`src/lib.rs:673`:

```rust
pub fn search_paths(mut self, paths: &'a [*const c_char]) -> Self {
    self.inner.searchPaths = paths.as_ptr();
    self.inner.searchPathCount = paths.len() as _;
    self
}
```

The function is safe to call, takes raw pointers it never validates, and stashes
them in a struct that `GlobalSession::create_session` (`src/lib.rs:204`) hands to
Slang, which dereferences every one. The idiomatic-looking call is a trap:

```rust
let search_paths = [CString::new("shaders").unwrap().as_ptr()];  // temporary dropped here
let desc = SessionDesc::default().search_paths(&search_paths);   // dangling, no unsafe
```

The `'a` on the slice ties the *pointer array* to the descriptor's lifetime but
says nothing about what the pointers point at. This is upstream issue #31: UB
reachable from entirely safe code.

It is also the last inconsistency in the crate's string handling. Every other
public string-taking API takes `&str` and owns the conversion internally —
`GlobalSession::find_profile` (`src/lib.rs:212`), and the whole `option!` macro
family feeding `CompilerOptions::push_str1`/`push_str2` (`src/lib.rs:751-769`).
The README's example (and `src/tests.rs:7,19,23`) has to teach the `CString` +
`as_ptr()` dance purely because of this one function.

The `i8` → `c_char` change on this branch (`src/lib.rs:673`, matching upstream
PR #37) fixed an unrelated aarch64 build break. It did not touch the soundness.

## Decisions taken

**Take `impl IntoIterator<Item: AsRef<str>>`, matching PR #34's signature.** It
accepts `["a", "b"]`, `&["a"]`, `Vec<String>`, and iterator chains, and it is the
shape upstream has converged on, which keeps a future reconciliation cheap.

**`AsRef<str>`, not `AsRef<Path>`.** Paths would be the more precise domain type,
but they drag in `OsStr` handling that differs per platform (Windows `OsStr` is
not byte-convertible on stable) for no gain — Slang takes UTF-8 `char*` either
way. This also matches laurooyen's stated direction in the issue #31 thread:
`&str` where possible, hidden allocation is acceptable, session creation is not
a hot path.

**Reuse `CompilerOptions`' storage pattern rather than PR #34's `CStringPtr`.**
PR #34 adds `src/c_string_ptr.rs`, a `#[repr(C)]` newtype over `*mut c_char` with
`CString::into_raw` in `From` and `CString::from_raw` in `Drop`, then casts
`Vec<CStringPtr>::as_ptr()` to `*const *const c_char`. That is sound, but it
leans on three things that need arguing: manual raw ownership, a layout
equivalence between `CStringPtr` and `*const c_char`, and a `#[repr(C)]`
annotation carrying real weight.

`CompilerOptions` already solves the identical problem without any of it
(`src/lib.rs:711`, `:751-769`): keep the `CString`s alive in a `Vec<CString>`
field, take `.as_ptr()` before moving each into the vector — the heap buffer does
not move — and store the raw pointer in the FFI struct. Doing the same here needs
no `unsafe` block at all.

**Panic on an interior nul, documented.** Consistent with `push_str1`
(`src/lib.rs:752`) and `find_profile` (`src/lib.rs:213`), both of which already
`unwrap()` their `CString::new`. Returning `Result` from one link of a builder
chain would force `?` into the middle of an otherwise-fluent API for an error
essentially nobody hits. Document it under a `# Panics` heading, as PR #34 does.

**Replaces rather than appends,** documented — same as PR #34.

**Fork-local, shaped to converge.** If upstream merges #34 first, reconcile to
the merged version rather than carrying a gratuitously different API; the
signature is deliberately identical so only the private storage differs.

## Plan

### 1. Give `SessionDesc` owned storage

`SessionDesc` (`src/lib.rs:640-644`) currently is:

```rust
#[repr(transparent)]
pub struct SessionDesc<'a> {
    inner: sys::slang_SessionDesc,
    _phantom: PhantomData<&'a ()>,
}
```

Add two fields and drop `#[repr(transparent)]` (no longer true) along with the
`Deref<Target = sys::slang_SessionDesc>` impl at `src/lib.rs:646-652` (which
only exists to serve one call site):

```rust
pub struct SessionDesc<'a> {
    inner: sys::slang_SessionDesc,
    /// Backing storage for `search_path_ptrs`; never read directly.
    _search_path_strings: Vec<CString>,
    /// The array `inner.searchPaths` points at.
    search_path_ptrs: Vec<*const c_char>,
    _phantom: PhantomData<&'a ()>,
}
```

Keep the `'a`: `targets` (`src/lib.rs:667`) and `options` (`src/lib.rs:679`) still
borrow. Add both fields to the `Default` impl at `src/lib.rs:654-663`.

### 2. Rewrite `search_paths`

```rust
/// Sets the search paths, replacing any previously set. Paths are copied into
/// this `SessionDesc`, which owns them for as long as it lives.
///
/// # Panics
///
/// Panics if any path contains an interior nul byte.
pub fn search_paths<P: AsRef<str>>(mut self, paths: impl IntoIterator<Item = P>) -> Self {
    self._search_path_strings = paths
        .into_iter()
        .map(|p| CString::new(p.as_ref()).expect("search path contains an interior nul byte"))
        .collect();

    self.search_path_ptrs = self._search_path_strings.iter().map(|s| s.as_ptr()).collect();

    self.inner.searchPaths = self.search_path_ptrs.as_ptr();
    self.inner.searchPathCount = self.search_path_ptrs.len() as _;

    self
}
```

Order matters: build both vectors to their final length *before* taking
`as_ptr()`, so no later reallocation invalidates `inner.searchPaths`. Moving the
`SessionDesc` afterwards is fine — moving a `Vec` does not move its heap buffer.
Assigning both fields wholesale (rather than pushing) also makes the
replace-not-append semantics fall out for free.

### 3. Fix the one `Deref` call site

`src/lib.rs:206`: `vcall!(self, createSession(&**desc, &mut session))` becomes
`vcall!(self, createSession(&desc.inner, &mut session))`.

### 4. Update the test and the README

`src/tests.rs`: delete the `search_path` `CString` (line 7) and the
`search_paths` array (line 19), and call `.search_paths(["shaders"])` (line 23).

`README.md`: the example teaches the raw-pointer form — remove the
`let search_path = std::ffi::CString::new("shaders/directory").unwrap();` and
`let search_paths = [search_path.as_ptr()];` lines and inline
`.search_paths(["shaders/directory"])`. This is a visible improvement to the
crate's front page and is half the reason to do this work.

### 5. Audit the rest of the public API for the same shape

Confirm nothing else takes raw pointers from safe callers. As of this branch the
only other `*const c_char` in `src/lib.rs` is `push_strings` (`:731`), which is
private and whose two callers own their `CString`s — no change needed. Record the
result so the audit is not repeated.

**Audit result (done):** `grep -rnE 'pub (unsafe )?fn [^(]*\([^)]*\*' src/` matches
nothing across `src/lib.rs` and all twelve `src/reflection/*.rs` modules — after
this change, no public function in the crate takes a raw pointer from a caller.
The remaining raw pointers are all outputs of the FFI (returned `*const c_char`
converted through `CStr` before crossing the public boundary) or private
plumbing. Do not repeat this audit; re-run the grep only if a new public
`fn` gains a pointer parameter.

## Verification

- `cargo test` on all three CI platforms. The existing `compile` test genuinely
  resolves `test.slang` through the search path, so a broken pointer array fails
  it rather than silently passing.
- Add tests covering `Vec<String>`, `&[&str]`, an iterator chain
  (`.iter().map(...)`), and an empty iterator (which must leave
  `searchPathCount == 0` with a non-dangling `searchPaths`).
- Add a test that calling `.search_paths(...)` twice uses only the second set.
- Compile-fail check: the issue #31 repro — building a `[*const c_char]` from
  dropped `CString`s — must no longer typecheck. A `trybuild` case is overkill;
  confirming manually and noting it in the commit message is enough.
- `cargo test --features serde` for completeness; this touches no serde surface.

## Risks

**Breaking change for consumers.** Anyone passing `&[*const c_char]` must switch
to strings. That is the entire point, the migration is one line, and this fork is
consumed by git tag rather than crates.io — so the blast radius is
`Giesch/vulkan-slang-renderer`. Bundle it into the same release as
`plans/01_resource_shape_bitflags.md` so consumers absorb one breaking bump.

**Divergence from upstream PR #34.** Same public signature, different private
storage, plus this fork keeps `search_paths` mandatory-free. If #34 lands
upstream the merge will conflict in `src/lib.rs`; resolve toward whichever
implementation is simpler at that point — the signatures already agree, so this
is a small conflict by construction.

**`_search_path_strings` is dead-read.** It exists only to own the buffers, and
nothing reads it, so it will draw a `dead_code` warning unless the underscore
prefix is kept. Do not "clean it up" — deleting it reintroduces exactly the bug
this plan closes. The doc comment above it says so.
