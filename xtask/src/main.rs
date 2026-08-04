//! Development tasks for this repository, run via `cargo run -p xtask -- <task>`
//! (or `just regen-bindings`). Never published, and not a dependency of
//! `shader-slang` or `shader-slang-sys` — bindgen and libclang stay out of
//! every consumer's build.

use std::path::{Path, PathBuf};

fn main() {
	match std::env::args().nth(1).as_deref() {
		Some("regen-bindings") => regenerate_bindings(),
		Some(task) => {
			eprintln!("xtask: unknown task '{task}'");
			eprintln!("tasks: regen-bindings");
			std::process::exit(1);
		}
		None => {
			eprintln!("xtask: no task given");
			eprintln!("tasks: regen-bindings");
			std::process::exit(1);
		}
	}
}

/// This crate lives at `<repo>/xtask`, so the workspace root is its parent.
fn repo_root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.expect("xtask must live in a subdirectory of the repository.")
		.to_path_buf()
}

/// Headers come from the tree `just fetch-static` extracts, which is also
/// where the libraries `build.rs` links come from, so the two cannot skew.
/// Release tags vendor archives instead of extracted trees; regenerating is a
/// `main`-branch operation, so this deliberately does not read `vendor/`.
fn header_dir(sys_dir: &Path) -> PathBuf {
	let platform = target_platform();
	let include = sys_dir.join("vendor-local").join(platform).join("include");
	assert!(
		include.is_dir(),
		"no slang headers at {}; run `just fetch-static` first",
		include.display()
	);
	include
}

/// Bindings are regenerated for the host, matching the platform names the
/// slang release archives use. CI runs this on every supported platform and
/// diffs the result, which is what proves one committed bindings.rs is valid
/// everywhere.
fn target_platform() -> &'static str {
	match (std::env::consts::OS, std::env::consts::ARCH) {
		("linux", "x86_64") => "linux-x86_64",
		("macos", "aarch64") => "macos-aarch64",
		("windows", "x86_64") => "windows-x86_64",
		(os, arch) => panic!("no prebuilt static slang library for {os}-{arch}"),
	}
}

/// Regenerates `slang-sys/src/bindings.rs` from the slang headers. A
/// maintainer operation rather than part of the build: the committed bindings
/// are authoritative, since headers and libraries ship together in the
/// vendored release and so cannot skew.
fn regenerate_bindings() {
	let sys_dir = repo_root().join("slang-sys");
	let include_dir = header_dir(&sys_dir).display().to_string();

	let bindings_path = sys_dir.join("src").join("bindings.rs");

	let builder = bindgen::builder()
		.header(format!("{include_dir}/slang.h").as_str())
		.clang_arg("-v")
		.clang_arg("-xc++")
		.clang_arg("-std=c++17")
		// The libraries are compiled with SLANG_STATIC; without it SLANG_API
		// resolves to __declspec(dllimport) on MSVC while the library exports
		// plain symbols.
		.clang_arg("-DSLANG_STATIC")
		// Pinned rather than left to bindgen's default, which probes
		// `rustc --version` — but only when running inside a build script
		// (it tests for CARGO_CFG_TARGET_ARCH), so from here it would silently
		// fall back to an older target whose latest edition is 2021. bindgen
		// passes the edition to rustfmt as its style edition, so an unpinned
		// value reformats the committed file. Pinning also keeps the output
		// independent of whichever rustc a CI runner happens to ship.
		.rust_target(bindgen::RustTarget::stable(85, 0).unwrap())
		.rust_edition(bindgen::RustEdition::Edition2024);

	builder
		.allowlist_function("spReflection.*")
		.allowlist_function("spComputeStringHash")
		.allowlist_function("slang_.*")
		.allowlist_type("slang.*")
		.allowlist_var("SLANG_.*")
		// Compiler/platform/processor introspection macros describe the
		// machine the bindings were generated on and vary per target; they are
		// excluded so the committed bindings are identical across targets.
		.blocklist_item("SLANG_(CLANG|VC|SNC|GHS|GCC|GCC_FAMILY)")
		.blocklist_item(
			"SLANG_(LINUX|OSX|IOS|ANDROID|WINRT|WIN64|WIN32|X360|XBOXONE|PS3|PS4|PSP2|WIIU|WASM)",
		)
		.blocklist_item("SLANG_(WINDOWS|APPLE|UNIX|MICROSOFT)_FAMILY")
		.blocklist_item("SLANG_PROCESSOR_.*")
		.blocklist_item("SLANG_(PTR_IS_32|PTR_IS_64|LITTLE_ENDIAN|BIG_ENDIAN|UNALIGNED_ACCESS)")
		.blocklist_item("SLANG_HAS_(EXCEPTIONS|MOVE_SEMANTICS|ENUM_CLASS|BACKTRACE)")
		.blocklist_item("SLANG_ENABLE_(DIRECTX|DXVK|VKD3D|DXGI_DEBUG|DXBC_SUPPORT|PIX)")
		// Declared without extern "C", so their #[link_name] bakes in
		// per-target C++ mangling (Apple prepends an underscore, MSVC mangles
		// differently). Unused by this crate's wrapper; excluded to keep the
		// bindings portable.
		.blocklist_function("spReflection_GetSession")
		.blocklist_function("slang_getEmbeddedCoreModule")
		.with_codegen_config(
			bindgen::CodegenConfig::FUNCTIONS
				| bindgen::CodegenConfig::TYPES
				| bindgen::CodegenConfig::VARS,
		)
		.parse_callbacks(Box::new(ParseCallback {}))
		.default_enum_style(bindgen::EnumVariation::Rust {
			non_exhaustive: false,
		})
		.constified_enum("SlangProfileID")
		.constified_enum("SlangCapabilityID")
		.vtable_generation(true)
		.layout_tests(false)
		.derive_copy(true)
		.generate()
		.expect("Couldn't generate bindings.")
		.write_to_file(&bindings_path)
		.expect("Couldn't write bindings.");

	normalize_enum_reprs(&bindings_path);
}

/// slang.h declares a few unscoped enums without a fixed underlying type.
/// MSVC gives those `int`, while the Itanium ABI picks `unsigned int` when no
/// enumerator is negative, so bindgen's `#[repr]` for them varies by target.
/// Normalize to the Itanium result so the committed bindings are
/// target-independent; both are 32-bit, so the ABI is unchanged.
fn normalize_enum_reprs(bindings_path: &std::path::Path) {
	const ITANIUM_U32_ENUMS: &[&str] = &[
		"_bindgen_ty_1",
		"_bindgen_ty_2",
		"_bindgen_ty_3",
		"slang__bindgen_ty_1",
		"SlangReflectionGenericArgType",
	];

	let generated = std::fs::read_to_string(bindings_path).expect("Couldn't read bindings.");
	let mut lines: Vec<&str> = generated.lines().collect();
	for i in 0..lines.len() {
		if lines[i] != "#[repr(i32)]" {
			continue;
		}
		// The enum declaration follows within a few lines, after attributes.
		let is_target = lines[i..lines.len().min(i + 4)].iter().any(|line| {
			line.strip_prefix("pub enum ")
				.map(|rest| ITANIUM_U32_ENUMS.contains(&rest.trim_end_matches(" {")))
				.unwrap_or(false)
		});
		if is_target {
			lines[i] = "#[repr(u32)]";
		}
	}
	std::fs::write(bindings_path, lines.join("\n") + "\n").expect("Couldn't write bindings.");
}

#[derive(Debug)]
struct ParseCallback {}

impl bindgen::callbacks::ParseCallbacks for ParseCallback {
	fn enum_variant_name(
		&self,
		enum_name: Option<&str>,
		original_variant_name: &str,
		_variant_value: bindgen::callbacks::EnumVariantValue,
	) -> Option<String> {
		let enum_name = enum_name?;

		// Map enum names to the part of their variant names that needs to be trimmed.
		// When an enum name is not in this map the code below will try to trim the enum name itself.
		let mut map = std::collections::HashMap::new();
		map.insert("SlangMatrixLayoutMode", "SlangMatrixLayout");
		map.insert("SlangCompileTarget", "Slang");

		let trim = map.get(enum_name).unwrap_or(&enum_name);
		let new_variant_name = pascal_case_from_snake_case(original_variant_name);
		let new_variant_name = new_variant_name.trim_start_matches(trim);
		Some(new_variant_name.to_string())
	}

	// The committed bindings are feature-independent: serde derives are baked
	// in behind cfg_attr rather than added only when regenerating with the
	// serde feature enabled.
	fn add_attributes(&self, info: &bindgen::callbacks::AttributeInfo<'_>) -> Vec<String> {
		if info.name.starts_with("Slang") && info.kind == bindgen::callbacks::TypeKind::Enum {
			return vec![
				r#"#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]"#
					.into(),
			];
		}
		vec![]
	}
}

/// Converts `snake_case` or `SNAKE_CASE` to `PascalCase`.
/// If the input is already in `PascalCase` it will be returned as is.
fn pascal_case_from_snake_case(snake_case: &str) -> String {
	let mut result = String::new();

	let should_lower = snake_case
		.chars()
		.filter(|c| c.is_alphabetic())
		.all(|c| c.is_uppercase());

	for part in snake_case.split('_') {
		for (i, c) in part.chars().enumerate() {
			if i == 0 {
				result.push(c.to_ascii_uppercase());
			} else if should_lower {
				result.push(c.to_ascii_lowercase());
			} else {
				result.push(c);
			}
		}
	}

	result
}
