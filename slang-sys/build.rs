use std::env;
use std::path::PathBuf;

fn main() {
	// When both features are enabled (Cargo features are additive, so any
	// dependent enabling default features can cause that), static wins.
	let static_lib = cfg!(feature = "static");

	#[cfg(feature = "static")]
	let vendored: Option<PathBuf> = {
		let tree = static_lib_tree();

		println!(
			"cargo:rustc-link-search=native={}",
			tree.join("lib").display()
		);
		// The release bundles everything (slang, compiler-core, core, miniz,
		// lz4, cmark-gfm, embedded glslang) into this one library.
		println!("cargo:rustc-link-lib=static=slang-static");

		// System libraries the slang release's own consumer link check proves
		// are needed. link-cplusplus supplies the C++ runtime.
		match env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
			"linux" => {
				println!("cargo:rustc-link-lib=m");
				println!("cargo:rustc-link-lib=pthread");
				println!("cargo:rustc-link-lib=dl");
			}
			"macos" => {
				println!("cargo:rustc-link-lib=m");
				println!("cargo:rustc-link-lib=pthread");
			}
			_ => {}
		}

		Some(tree)
	};

	#[cfg(not(feature = "static"))]
	let vendored: Option<PathBuf> = {
		println!("cargo:rerun-if-env-changed=SLANG_DIR");
		println!("cargo:rerun-if-env-changed=SLANG_LIB_DIR");
		println!("cargo:rerun-if-env-changed=VULKAN_SDK");

		let lib_dir = if let Ok(dir) = env::var("SLANG_LIB_DIR") {
			dir
		} else if let Ok(dir) = env::var("SLANG_DIR") {
			format!("{dir}/lib")
		} else if let Ok(dir) = env::var("VULKAN_SDK") {
			format!("{dir}/lib")
		} else {
			panic!("The environment variable SLANG_LIB_DIR, SLANG_DIR, or VULKAN_SDK must be set");
		};

		if !lib_dir.is_empty() {
			println!("cargo:rustc-link-search=native={lib_dir}");
		}

		println!("cargo:rustc-link-lib=dylib=slang");

		None
	};

	#[cfg(feature = "regenerate-bindings")]
	regenerate_bindings(vendored.as_deref(), static_lib);
	#[cfg(not(feature = "regenerate-bindings"))]
	drop(vendored);
}

/// Locates — extracting if necessary — the prebuilt static library tree for
/// the current target. `vendor/` holds the release `.tar.xz` archives on
/// release tags, expanded into `OUT_DIR` on first build; `vendor-local/`
/// holds trees already extracted by `just fetch-static` on `main`.
#[cfg(feature = "static")]
fn static_lib_tree() -> PathBuf {
	let platform = target_platform();
	let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

	if let Some(archive) = vendored_archive(&manifest_dir.join("vendor"), platform) {
		println!("cargo:rerun-if-changed={}", archive.display());
		return extract_archive(&archive, &manifest_dir, platform);
	}

	let local = manifest_dir.join("vendor-local").join(platform);
	if local.is_dir() {
		println!("cargo:rerun-if-changed={}", local.display());
		return local;
	}

	panic!(
		"static slang libraries not found under {}; run `just fetch-static`, \
		 or depend on a release tag that vendors the archives",
		manifest_dir.display()
	);
}

#[cfg(feature = "static")]
fn target_platform() -> &'static str {
	let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
	let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
	match (os.as_str(), arch.as_str()) {
		("linux", "x86_64") => "linux-x86_64",
		("macos", "aarch64") => "macos-aarch64",
		("windows", "x86_64") => "windows-x86_64",
		_ => panic!(
			"no prebuilt static slang library for target '{}'; static libs ship for \
			 x86_64-unknown-linux-gnu, aarch64-apple-darwin, and x86_64-pc-windows-msvc",
			env::var("TARGET").unwrap_or_default()
		),
	}
}

/// Finds the vendored release archive for `platform`, matching any slang
/// version so the version stays pinned in one place (the justfile and the
/// SHA-256 manifest).
#[cfg(feature = "static")]
fn vendored_archive(vendor_dir: &std::path::Path, platform: &str) -> Option<PathBuf> {
	let suffix = format!("-{platform}.tar.xz");
	std::fs::read_dir(vendor_dir)
		.ok()?
		.filter_map(|entry| entry.ok())
		.map(|entry| entry.path())
		.find(|path| {
			path.file_name()
				.and_then(|name| name.to_str())
				.is_some_and(|name| name.starts_with("slang-static-") && name.ends_with(&suffix))
		})
}

/// Verifies the archive against the committed manifest and unpacks it into
/// `OUT_DIR`, stripping the archive's top-level directory. Idempotent per
/// `OUT_DIR` via a marker file, so the cost is paid once per profile.
#[cfg(feature = "static")]
fn extract_archive(
	archive: &std::path::Path,
	manifest_dir: &std::path::Path,
	platform: &str,
) -> PathBuf {
	let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
	let dest = out_dir.join("slang-static").join(platform);
	let marker = dest.join(".complete");
	if marker.exists() {
		return dest;
	}

	verify_archive_hash(archive, manifest_dir);

	if dest.exists() {
		std::fs::remove_dir_all(&dest).expect("Couldn't clear the extraction directory.");
	}
	std::fs::create_dir_all(&dest).expect("Couldn't create the extraction directory.");

	// Decompress to a temporary tar on disk rather than in memory; the
	// Windows archive expands to ~490 MB.
	let tar_path = out_dir.join("slang-static.tar");
	{
		let mut input = std::io::BufReader::new(
			std::fs::File::open(archive).expect("Couldn't open the vendored archive."),
		);
		let mut output = std::io::BufWriter::new(
			std::fs::File::create(&tar_path).expect("Couldn't create the temporary tar."),
		);
		lzma_rs::xz_decompress(&mut input, &mut output)
			.expect("Couldn't decompress the vendored archive.");
	}

	let tar_file = std::fs::File::open(&tar_path).expect("Couldn't open the temporary tar.");
	let mut tar = tar::Archive::new(tar_file);
	for entry in tar.entries().expect("Couldn't read the temporary tar.") {
		let mut entry = entry.expect("Couldn't read a tar entry.");
		let path = entry
			.path()
			.expect("Couldn't read a tar entry path.")
			.into_owned();
		let stripped: PathBuf = path.components().skip(1).collect();
		if stripped.as_os_str().is_empty() {
			continue;
		}
		entry
			.unpack(dest.join(stripped))
			.expect("Couldn't unpack a tar entry.");
	}
	std::fs::remove_file(&tar_path).ok();

	std::fs::File::create(&marker).expect("Couldn't write the extraction marker.");
	dest
}

/// The committed manifest pins the SHA-256 of each published release asset;
/// failing loudly here keeps the provenance chain from the published release
/// to the extracted libraries intact.
#[cfg(feature = "static")]
fn verify_archive_hash(archive: &std::path::Path, manifest_dir: &std::path::Path) {
	use sha2::Digest;

	let name = archive.file_name().unwrap().to_str().unwrap();
	let manifest_path = manifest_dir.join("static-release.sha256");
	let manifest =
		std::fs::read_to_string(&manifest_path).expect("Couldn't read static-release.sha256.");
	let expected = manifest
		.lines()
		.find_map(|line| {
			let (hash, file) = line.split_once("  ")?;
			(file.trim() == name).then(|| hash.trim().to_string())
		})
		.unwrap_or_else(|| {
			panic!(
				"no pinned SHA-256 for {name} in {}",
				manifest_path.display()
			)
		});

	let mut file = std::fs::File::open(archive).expect("Couldn't open the vendored archive.");
	let mut hasher = sha2::Sha256::new();
	std::io::copy(&mut file, &mut hasher).expect("Couldn't hash the vendored archive.");
	let actual = format!("{:x}", hasher.finalize());
	assert_eq!(
		actual, expected,
		"SHA-256 mismatch for {name}: the vendored archive does not match the pinned release asset"
	);
}

/// Regenerates `src/bindings.rs` from the slang headers. Opt-in because the
/// committed bindings are authoritative: headers and libraries ship together
/// in the vendored release, so the two cannot skew.
#[cfg(feature = "regenerate-bindings")]
fn regenerate_bindings(vendored: Option<&std::path::Path>, static_lib: bool) {
	let include_dir = match vendored {
		Some(tree) => tree.join("include").display().to_string(),
		None => {
			println!("cargo:rerun-if-env-changed=SLANG_INCLUDE_DIR");
			if let Ok(dir) = env::var("SLANG_INCLUDE_DIR") {
				dir
			} else if let Ok(dir) = env::var("SLANG_DIR") {
				format!("{dir}/include")
			} else if let Ok(dir) = env::var("VULKAN_SDK") {
				format!("{dir}/include/slang")
			} else {
				panic!(
					"The environment variable SLANG_INCLUDE_DIR, SLANG_DIR, or VULKAN_SDK must be set"
				);
			}
		}
	};

	let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
	let bindings_path = manifest_dir.join("src").join("bindings.rs");

	let mut builder = bindgen::builder()
		.header(format!("{include_dir}/slang.h").as_str())
		.clang_arg("-v")
		.clang_arg("-xc++")
		.clang_arg("-std=c++17");

	if static_lib {
		// The libraries are compiled with SLANG_STATIC; without it SLANG_API
		// resolves to __declspec(dllimport) on MSVC while the library exports
		// plain symbols.
		builder = builder.clang_arg("-DSLANG_STATIC");
	}

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
#[cfg(feature = "regenerate-bindings")]
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

#[cfg(feature = "regenerate-bindings")]
#[derive(Debug)]
struct ParseCallback {}

#[cfg(feature = "regenerate-bindings")]
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
#[cfg(feature = "regenerate-bindings")]
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
