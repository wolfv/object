use object::build::macho::Builder;
use object::macho;
use object::write::{MachODylib, MachOEntryPoint, MachOLoadDylinker, MachORpath};
use object::{write, Architecture, BinaryFormat, Endianness};

/// Test basic round-trip: create executable → write → read back with Builder
#[test]
fn builder_round_trip_executable() {
    // Create an executable using write::Object
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_EXECUTE);
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));
    object.add_macho_rpath(MachORpath::from_str("@executable_path/../Frameworks"));
    object.add_macho_rpath(MachORpath::from_str("@loader_path/lib"));

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0x90; 16], 1);

    let bytes = object.write().unwrap();

    // Read it back with Builder
    let builder = Builder::read(&*bytes).unwrap();

    // Verify header
    assert_eq!(builder.header.file_type, macho::MH_EXECUTE);
    assert_eq!(builder.is_64, true);
    assert_eq!(builder.architecture, Architecture::X86_64);

    // Verify load commands
    assert!(builder.has_load_dylinker());
    assert!(builder.has_entry_point());
    assert_eq!(builder.rpaths().count(), 2);

    // Verify RPATHs
    let rpaths: Vec<_> = builder.rpaths().collect();
    assert_eq!(rpaths.len(), 2);
    assert_eq!(rpaths[0], b"@executable_path/../Frameworks");
    assert_eq!(rpaths[1], b"@loader_path/lib");
}

/// Test modifying RPATHs: read → modify → write → verify
#[test]
fn builder_modify_rpaths() {
    // Create an executable with one rpath
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_EXECUTE);
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));
    object.add_macho_rpath(MachORpath::from_str("@executable_path/../Frameworks"));

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    let bytes = object.write().unwrap();

    // Read with Builder
    let mut builder = Builder::read(&*bytes).unwrap();

    // Verify initial state
    assert_eq!(builder.rpaths().count(), 1);

    // Add a new rpath
    builder.add_rpath("@loader_path/lib");

    // Remove the old rpath
    builder.remove_rpath("@executable_path/../Frameworks");

    // Verify modifications
    let rpaths: Vec<_> = builder.rpaths().collect();
    assert_eq!(rpaths.len(), 1);
    assert_eq!(rpaths[0], b"@loader_path/lib");

    // Write back
    let modified_bytes = builder.write().unwrap();

    // Read again and verify
    let builder2 = Builder::read(&*modified_bytes).unwrap();
    let rpaths2: Vec<_> = builder2.rpaths().collect();
    assert_eq!(rpaths2.len(), 1);
    assert_eq!(rpaths2[0], b"@loader_path/lib");
}

/// Test round-trip for dylib with install name
#[test]
fn builder_round_trip_dylib() {
    // Create a dylib
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_DYLIB);

    let mut id_dylib = MachODylib::from_str("@rpath/MyLibrary.dylib");
    id_dylib.current_version = MachODylib::encode_version(2, 5, 1);
    id_dylib.compatibility_version = MachODylib::encode_version(1, 0, 0);
    object.set_macho_id_dylib(id_dylib);

    object.add_macho_load_dylib(MachODylib::from_str("/usr/lib/libSystem.B.dylib"));
    object.add_macho_load_dylib(MachODylib::from_str("@rpath/Other.framework/Other"));

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    let bytes = object.write().unwrap();

    // Read with Builder
    let builder = Builder::read(&*bytes).unwrap();

    // Verify header
    assert_eq!(builder.header.file_type, macho::MH_DYLIB);

    // Verify install name
    assert_eq!(builder.install_name().unwrap(), b"@rpath/MyLibrary.dylib");

    // Verify dependencies
    let deps: Vec<_> = builder.dependencies().collect();
    assert_eq!(deps.len(), 2);
    assert_eq!(deps[0], b"/usr/lib/libSystem.B.dylib");
    assert_eq!(deps[1], b"@rpath/Other.framework/Other");
}

/// Test modifying install name
#[test]
fn builder_modify_install_name() {
    // Create a dylib
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_DYLIB);
    object.set_macho_id_dylib(MachODylib::from_str("@rpath/OldName.dylib"));

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    let bytes = object.write().unwrap();

    // Read with Builder
    let mut builder = Builder::read(&*bytes).unwrap();

    // Verify initial name
    assert_eq!(builder.install_name().unwrap(), b"@rpath/OldName.dylib");

    // Change install name
    builder.set_install_name(
        "@rpath/NewName.dylib",
        MachODylib::encode_version(3, 0, 0),
        MachODylib::encode_version(1, 0, 0),
    );

    // Write back
    let modified_bytes = builder.write().unwrap();

    // Read again and verify
    let builder2 = Builder::read(&*modified_bytes).unwrap();
    assert_eq!(builder2.install_name().unwrap(), b"@rpath/NewName.dylib");
}

/// Test modifying dependencies
#[test]
fn builder_modify_dependencies() {
    // Create an executable with dependencies
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_EXECUTE);
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));

    object.add_macho_load_dylib(MachODylib::from_str("/usr/lib/libSystem.B.dylib"));
    object.add_macho_load_dylib(MachODylib::from_str("@rpath/OldLib.dylib"));

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    let bytes = object.write().unwrap();

    // Read with Builder
    let mut builder = Builder::read(&*bytes).unwrap();

    // Verify initial dependencies
    let deps: Vec<_> = builder.dependencies().collect();
    assert_eq!(deps.len(), 2);

    // Add a new dependency
    builder.add_dependency("@rpath/NewLib.dylib");

    // Remove old library
    builder.remove_dependency("@rpath/OldLib.dylib");

    // Verify modifications
    let deps: Vec<_> = builder.dependencies().collect();
    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&b"/usr/lib/libSystem.B.dylib".as_ref()));
    assert!(deps.contains(&b"@rpath/NewLib.dylib".as_ref()));

    // Write back
    let modified_bytes = builder.write().unwrap();

    // Read again and verify
    let builder2 = Builder::read(&*modified_bytes).unwrap();
    let deps2: Vec<_> = builder2.dependencies().collect();
    assert_eq!(deps2.len(), 2);
    assert!(deps2.contains(&b"/usr/lib/libSystem.B.dylib".as_ref()));
    assert!(deps2.contains(&b"@rpath/NewLib.dylib".as_ref()));
}

/// Test complex modification scenario: multiple changes at once
#[test]
fn builder_complex_modifications() {
    // Create a dylib
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_DYLIB);
    object.set_macho_id_dylib(MachODylib::from_str("@rpath/Original.dylib"));
    object.add_macho_rpath(MachORpath::from_str("@loader_path"));
    object.add_macho_load_dylib(MachODylib::from_str(
        "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation",
    ));

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    let bytes = object.write().unwrap();

    // Read with Builder
    let mut builder = Builder::read(&*bytes).unwrap();

    // Perform multiple modifications
    builder.set_install_name(
        "@rpath/Modified.dylib",
        MachODylib::encode_version(5, 2, 3),
        MachODylib::encode_version(2, 0, 0),
    );

    builder.add_rpath("@executable_path/../Frameworks");
    builder.add_rpath("/usr/local/lib");

    builder.add_dependency("@rpath/Dependency1.dylib");
    builder.add_dependency("@rpath/Dependency2.dylib");

    // Write back
    let modified_bytes = builder.write().unwrap();

    // Verify all changes
    let builder2 = Builder::read(&*modified_bytes).unwrap();

    assert_eq!(builder2.header.file_type, macho::MH_DYLIB);
    assert_eq!(builder2.install_name().unwrap(), b"@rpath/Modified.dylib");

    let rpaths: Vec<_> = builder2.rpaths().collect();
    assert_eq!(rpaths.len(), 3);
    assert!(rpaths.contains(&b"@loader_path".as_ref()));
    assert!(rpaths.contains(&b"@executable_path/../Frameworks".as_ref()));
    assert!(rpaths.contains(&b"/usr/local/lib".as_ref()));

    let deps: Vec<_> = builder2.dependencies().collect();
    assert_eq!(deps.len(), 3);
    assert!(deps
        .contains(&b"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation".as_ref()));
    assert!(deps.contains(&b"@rpath/Dependency1.dylib".as_ref()));
    assert!(deps.contains(&b"@rpath/Dependency2.dylib".as_ref()));
}

/// Test reading a minimal object file (MH_OBJECT)
#[test]
fn builder_read_object_file() {
    // Create a simple object file
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    // Don't set file type - should default to MH_OBJECT
    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0x90; 32], 1);

    let bytes = object.write().unwrap();

    // Read with Builder
    let builder = Builder::read(&*bytes).unwrap();

    // Verify it's recognized as an object file
    assert_eq!(builder.header.file_type, macho::MH_OBJECT);
    assert_eq!(builder.is_64, true);
    assert_eq!(builder.architecture, Architecture::X86_64);

    // Object files shouldn't have these load commands
    assert!(!builder.has_load_dylinker());
    assert!(!builder.has_entry_point());
}
