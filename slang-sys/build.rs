use std::env;
use std::path::{Path, PathBuf};

fn main() {
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
}

/// Locates — extracting if necessary — the prebuilt static library tree for
/// the current target. `vendor/` holds the release `.tar.xz` archives on
/// release tags, expanded into `OUT_DIR` on first build; `vendor-local/`
/// holds trees already extracted by `just fetch-static` on `main`.
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
fn vendored_archive(vendor_dir: &Path, platform: &str) -> Option<PathBuf> {
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
fn extract_archive(archive: &Path, manifest_dir: &Path, platform: &str) -> PathBuf {
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
fn verify_archive_hash(archive: &Path, manifest_dir: &Path) {
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
