<div align="center">

# shader-slang
**Rust bindings for the [Slang](https://github.com/shader-slang/slang/) shader language compiler**

</div>

Supports both the modern compilation and reflection API.

Currently mostly reflects the needs of our own [engine](https://github.com/FloatyMonkey/engine) but contributions are more than welcome.

## Example

```rust
let global_session = slang::GlobalSession::new().unwrap();

let search_path = std::ffi::CString::new("shaders/directory").unwrap();

// All compiler options are available through this builder.
let session_options = slang::CompilerOptions::default()
	.optimization(slang::OptimizationLevel::High)
	.matrix_layout_row(true);

let target_desc = slang::TargetDesc::default()
	.format(slang::CompileTarget::Spirv)
	.profile(global_session.find_profile("glsl_450"));

let targets = [target_desc];
let search_paths = [search_path.as_ptr()];

let session_desc = slang::SessionDesc::default()
	.targets(&targets)
	.search_paths(&search_paths)
	.options(&session_options);

let session = global_session.create_session(&session_desc).unwrap();
let module = session.load_module("filename.slang").unwrap();
let entry_point = module.find_entry_point_by_name("main").unwrap();

let program = session
	.create_composite_component_type(&[module.into(), entry_point.into()])
	.unwrap();

let linked_program = program.link().unwrap();

// Entry point to the reflection API.
let reflection = linked_program.layout(0).unwrap();

let shader_bytecode = linked_program.entry_point_code(0, 0).unwrap();
```

## Installation

This library links prebuilt static Slang libraries that are vendored into this
repository on release tags — no Slang installation, no environment variables,
no dynamic linking. Depend on a release tag, not on `main`:

```toml
[dependencies]
shader-slang = { git = "https://github.com/Giesch/slang-rs", tag = "<release tag>" }
```

Pin a `tag`, not a `branch` or bare `rev`: the static libs exist only in
release-tag commits, which keeps them out of `main`'s history and means Cargo
only transfers one release's set. On first build, `build.rs` verifies the
vendored archive against its pinned SHA-256 and expands it into the build
directory — once per profile; note the Windows library is ~490 MB expanded.

Static libs ship for these targets, built by
[`Giesch/slang`](https://github.com/Giesch/slang)'s `release-static.yml`:

| Rust target triple | notes |
| --- | --- |
| `x86_64-unknown-linux-gnu` | needs glibc ≥ 2.28 |
| `aarch64-apple-darwin` | macOS deployment target 13.0 |
| `x86_64-pc-windows-msvc` | built against the DLL CRT (`MultiThreadedDLL`), matching Rust's default — do not enable `+crt-static` |

When working on this repository itself (or building `main`), run
`just fetch-static` first: it downloads the pinned Slang release, verifies it
against the committed SHA-256 manifest, and extracts it into a gitignored
directory that `build.rs` picks up.

### Bindings

Generated bindings are checked in, so the build needs neither bindgen nor
libclang. Headers and libraries ship together in the vendored release, so the
committed bindings are authoritative and cannot skew from the linked library.

To regenerate them after bumping the pinned Slang release, run
`just fetch-static` and then `just regen-bindings`. That runs the `xtask`
package, which is where bindgen lives — it is never published and is not a
dependency of either library crate, so bindgen and libclang stay out of every
consumer's build. Regenerating does need libclang locally.

One `bindings.rs` is committed for all three targets. Keeping it
target-independent is deliberate: the platform introspection macros are
blocklisted, the enum `#[repr]`s are normalized to the Itanium result (MSVC
gives unfixed unscoped enums `int` where the Itanium ABI gives `unsigned int`;
both are 32-bit, so the ABI is unchanged), and the bindgen Rust target and
edition are pinned so output does not vary with the local toolchain. CI
regenerates on Linux, macOS, and Windows and diffs the result, which is what
proves the single committed file is valid everywhere.

## Credits

Maintained by Lauro Oyen ([@laurooyen](https://github.com/laurooyen)).

Licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
