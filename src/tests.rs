use crate as slang;

#[test]
fn compile() {
    let global_session = slang::GlobalSession::new().unwrap();

    let search_path = std::ffi::CString::new("shaders").unwrap();

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
    let module = session.load_module("test.slang").unwrap();
    let entry_point = module.find_entry_point_by_name("main").unwrap();

    let program = session
        .create_composite_component_type(&[module.into(), entry_point.into()])
        .unwrap();

    let linked_program = program.link().unwrap();

    // Entry point to the reflection API.
    let reflection = linked_program.layout(0).unwrap();
    assert_eq!(reflection.entry_point_count(), 1);
    assert_eq!(reflection.parameter_count(), 6);

    let shader_bytecode = linked_program.entry_point_code(0, 0).unwrap();
    assert_ne!(shader_bytecode.as_slice().len(), 0);
}

/// The flag-carrying parameters in `test.slang` reflect as bit patterns that
/// `slang.h` never names as enumerators — `combined_sampler` is the 0x102 from
/// FloatyMonkey/slang-rs#28 — so this is the end-to-end check that the shapes
/// arrive intact and decompose.
#[test]
fn resource_shapes() {
    use slang::{BaseBindingType, BaseShape};

    let global_session = slang::GlobalSession::new().unwrap();

    let search_path = std::ffi::CString::new("shaders").unwrap();

    let target_desc = slang::TargetDesc::default()
        .format(slang::CompileTarget::Spirv)
        .profile(global_session.find_profile("glsl_450"));

    let targets = [target_desc];
    let search_paths = [search_path.as_ptr()];

    let session_desc = slang::SessionDesc::default()
        .targets(&targets)
        .search_paths(&search_paths);

    let session = global_session.create_session(&session_desc).unwrap();
    let module = session.load_module("test.slang").unwrap();
    let entry_point = module.find_entry_point_by_name("main").unwrap();

    let program = session
        .create_composite_component_type(&[module.into(), entry_point.into()])
        .unwrap();

    let linked_program = program.link().unwrap();
    let reflection = linked_program.layout(0).unwrap();

    let shape = |name: &str| {
        reflection
            .parameters()
            .find(|p| p.name() == Some(name))
            .unwrap_or_else(|| panic!("no parameter named {name}"))
            .type_layout()
            .unwrap()
            .resource_shape()
            .unwrap()
    };

    let structured = shape("input_0");
    assert_eq!(structured.base(), BaseShape::StructuredBuffer);
    assert!(!structured.is_array());
    assert!(!structured.is_combined());

    let tex_array = shape("tex_array");
    assert_eq!(tex_array.base(), BaseShape::Texture2D);
    assert!(tex_array.is_array());
    assert!(!tex_array.is_combined());
    assert!(!tex_array.is_multisample());

    let combined = shape("combined_sampler");
    assert_eq!(combined.base(), BaseShape::Texture2D);
    assert!(combined.is_combined());
    assert!(!combined.is_shadow());
    assert!(!combined.is_array());
    assert_eq!(format!("{combined:?}"), "Texture2D|COMBINED");

    let shadow = shape("shadow_sampler");
    assert_eq!(shadow.base(), BaseShape::Texture2D);
    assert!(shadow.is_combined());
    assert!(shadow.is_shadow());
    assert_eq!(format!("{shadow:?}"), "Texture2D|SHADOW|COMBINED");

    let global_params = reflection.global_params_type_layout().unwrap();
    let binding_types: Vec<_> = (0..global_params.binding_range_count())
        .map(|i| global_params.binding_range_type(i))
        .collect();

    let bases: Vec<_> = binding_types.iter().map(|b| b.base()).collect();
    assert_eq!(
        bases,
        vec![
            BaseBindingType::RawBuffer,
            BaseBindingType::RawBuffer,
            BaseBindingType::RawBuffer,
            BaseBindingType::Texture,
            BaseBindingType::CombinedTextureSampler,
            BaseBindingType::CombinedTextureSampler,
        ]
    );

    let mutable: Vec<_> = binding_types.iter().map(|b| b.is_mutable()).collect();
    assert_eq!(mutable, vec![false, false, true, false, false, false]);
    assert_eq!(format!("{:?}", binding_types[2]), "RawBuffer|MUTABLE");
}

#[test]
fn bindless_space_index() {
    let global_session = slang::GlobalSession::new().unwrap();

    let search_path = std::ffi::CString::new("shaders").unwrap();

    // `BindlessSpaceIndex` is a target option, so it goes on the `TargetDesc`.
    let target_options = slang::CompilerOptions::default().bindless_space_index(3);

    let target_desc = slang::TargetDesc::default()
        .format(slang::CompileTarget::Spirv)
        .profile(global_session.find_profile("glsl_450"))
        .options(&target_options);

    let targets = [target_desc];
    let search_paths = [search_path.as_ptr()];

    let session_desc = slang::SessionDesc::default()
        .targets(&targets)
        .search_paths(&search_paths);

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
