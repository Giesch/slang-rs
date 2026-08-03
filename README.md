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

### Static linking (recommended)

The `static` feature links prebuilt static Slang libraries that are vendored
into this repository on release tags — no Slang installation, no environment
variables. Depend on a release tag, not on `main`:

```toml
[dependencies]
shader-slang = { git = "https://github.com/Giesch/slang-rs", tag = "<release tag>", default-features = false, features = ["static"] }
```

Pin a `tag`, not a `branch` or bare `rev`: the static libs exist only in
release-tag commits, which keeps them out of `main`'s history and means Cargo
only transfers one release's set.

Static libs ship for these targets, built by
[`Giesch/slang`](https://github.com/Giesch/slang)'s `release-static.yml`:

| Rust target triple | notes |
| --- | --- |
| `x86_64-unknown-linux-gnu` | needs glibc ≥ 2.28 |
| `aarch64-apple-darwin` | macOS deployment target 13.0 |
| `x86_64-pc-windows-msvc` | built against the DLL CRT (`MultiThreadedDLL`), matching Rust's default — do not enable `+crt-static` |

When working on this repository itself (or building `main` with
`--features static`), run `just fetch-static` first: it downloads the pinned
Slang release, verifies it against the committed SHA-256 manifest, and
extracts it into a gitignored directory that `build.rs` picks up.

### Dynamic linking

The default `dynamic` feature links against an existing Slang installation.
Point this library at one via environment variables: install the
[LunarG Vulkan SDK](https://vulkan.lunarg.com) (sets `VULKAN_SDK`, picked up
automatically), or download Slang from their
[releases page](https://github.com/shader-slang/slang/releases) and set
`SLANG_DIR` to the Slang directory. To specify the `lib` directory separately,
set `SLANG_LIB_DIR`.

At runtime, copy `slang.dll` to your executable's directory. To compile to
DXIL bytecode, also copy `dxil.dll` and `dxcompiler.dll` from the
[Microsoft DirectXShaderCompiler](https://github.com/microsoft/DirectXShaderCompiler/releases)
to your executable's directory.

### Bindings

Generated bindings are checked in, so the default build needs neither bindgen
nor libclang. To regenerate after bumping the pinned Slang release, run
`just regen-bindings` (or build with the `regenerate-bindings` feature, which
also requires `SLANG_INCLUDE_DIR`/`SLANG_DIR`/`VULKAN_SDK` when used with
dynamic linking).

## Credits

Maintained by Lauro Oyen ([@laurooyen](https://github.com/laurooyen)).

Licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
