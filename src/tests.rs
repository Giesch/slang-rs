use crate as slang;

#[test]
fn compile() {
    let global_session = slang::GlobalSession::new().unwrap();

    // All compiler options are available through this builder.
    let session_options = slang::CompilerOptions::default()
        .optimization(slang::OptimizationLevel::High)
        .matrix_layout_row(true);

    let target_desc = slang::TargetDesc::default()
        .format(slang::CompileTarget::Spirv)
        .profile(global_session.find_profile("glsl_450"));

    let targets = [target_desc];

    let session_desc = slang::SessionDesc::default()
        .targets(&targets)
        .search_paths(["shaders"])
        .options(&session_options);

    let session = global_session.create_session(&session_desc).unwrap();
    let module = session.load_module("test.slang").unwrap();
    let entry_point = module.find_entry_point_by_name("main").unwrap();

    let program = session
        .create_composite_component_type(&[module.into(), entry_point.into()])
        .unwrap();

    let linked_program = program.link().unwrap();

    // Entry point to the reflection API.
    let reflection = linked_program.layout(0).unwrap();
    assert_eq!(reflection.entry_point_count(), 1);
    assert_eq!(reflection.parameter_count(), 3);

    let shader_bytecode = linked_program.entry_point_code(0, 0).unwrap();
    assert_ne!(shader_bytecode.as_slice().len(), 0);
}

#[test]
fn bindless_space_index() {
    let global_session = slang::GlobalSession::new().unwrap();

    // `BindlessSpaceIndex` is a target option, so it goes on the `TargetDesc`.
    let target_options = slang::CompilerOptions::default().bindless_space_index(3);

    let target_desc = slang::TargetDesc::default()
        .format(slang::CompileTarget::Spirv)
        .profile(global_session.find_profile("glsl_450"))
        .options(&target_options);

    let targets = [target_desc];

    let session_desc = slang::SessionDesc::default()
        .targets(&targets)
        .search_paths(["shaders"]);

    let session = global_session.create_session(&session_desc).unwrap();
    let module = session.load_module("bindless.slang").unwrap();
    let entry_point = module.find_entry_point_by_name("main").unwrap();

    let program = session
        .create_composite_component_type(&[module.into(), entry_point.into()])
        .unwrap();

    let linked_program = program.link().unwrap();

    let reflection = linked_program.layout(0).unwrap();
    assert_eq!(reflection.bindless_space_index(), 3);

    let shader_bytecode = linked_program.entry_point_code(0, 0).unwrap();
    assert_ne!(shader_bytecode.as_slice().len(), 0);
}

/// Reads back what a `SessionDesc` will actually hand to Slang.
fn search_paths_as_seen_by_slang(desc: &slang::SessionDesc) -> Vec<String> {
    let count = desc.inner.searchPathCount as usize;
    assert!(!desc.inner.searchPaths.is_null());

    (0..count)
        .map(|i| unsafe {
            let ptr = *desc.inner.searchPaths.add(i);
            std::ffi::CStr::from_ptr(ptr).to_str().unwrap().to_owned()
        })
        .collect()
}

#[test]
fn search_paths_accepts_various_iterables() {
    let owned: Vec<String> = vec!["a".to_owned(), "b".to_owned()];
    let desc = slang::SessionDesc::default().search_paths(owned);
    assert_eq!(search_paths_as_seen_by_slang(&desc), ["a", "b"]);

    let borrowed: &[&str] = &["a", "b"];
    let desc = slang::SessionDesc::default().search_paths(borrowed);
    assert_eq!(search_paths_as_seen_by_slang(&desc), ["a", "b"]);

    let desc = slang::SessionDesc::default().search_paths(["a", "b"]);
    assert_eq!(search_paths_as_seen_by_slang(&desc), ["a", "b"]);

    let roots = ["x", "y"];
    let desc =
        slang::SessionDesc::default().search_paths(roots.iter().map(|r| format!("{r}/shaders")));
    assert_eq!(
        search_paths_as_seen_by_slang(&desc),
        ["x/shaders", "y/shaders"]
    );
}

#[test]
fn search_paths_empty_leaves_a_valid_array() {
    let desc = slang::SessionDesc::default().search_paths(Vec::<String>::new());

    assert_eq!(desc.inner.searchPathCount, 0);
    assert!(!desc.inner.searchPaths.is_null());
    assert!(search_paths_as_seen_by_slang(&desc).is_empty());
}

#[test]
fn search_paths_replaces_rather_than_appends() {
    let desc = slang::SessionDesc::default()
        .search_paths(["first", "also-first"])
        .search_paths(["second"]);

    assert_eq!(search_paths_as_seen_by_slang(&desc), ["second"]);
}

#[test]
fn search_paths_survive_a_move() {
    let desc = slang::SessionDesc::default().search_paths(["shaders"]);
    let moved = Box::new(desc);

    assert_eq!(search_paths_as_seen_by_slang(&moved), ["shaders"]);
}

#[test]
#[should_panic(expected = "nul byte")]
fn search_paths_panics_on_interior_nul() {
    let _ = slang::SessionDesc::default().search_paths(["sha\0ders"]);
}

/// The temporary-`CString` pattern from the old API used to compile and dangle;
/// now the paths are copied and outlive their source strings.
#[test]
fn compile_with_owned_search_paths() {
    let global_session = slang::GlobalSession::new().unwrap();

    let target_desc = slang::TargetDesc::default()
        .format(slang::CompileTarget::Spirv)
        .profile(global_session.find_profile("glsl_450"));

    let targets = [target_desc];

    let session_desc = {
        let search_paths = vec!["shaders".to_owned()];
        slang::SessionDesc::default()
            .targets(&targets)
            .search_paths(search_paths)
    };

    let session = global_session.create_session(&session_desc).unwrap();
    let module = session.load_module("test.slang").unwrap();
    assert!(module.find_entry_point_by_name("main").is_some());
}
