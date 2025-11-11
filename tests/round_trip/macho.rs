use object::read::macho::{LoadCommandVariant, MachHeader};
use object::read::{Object, ObjectSection};
use object::write::{
    MachODylib, MachOEntryPoint, MachOLoadDylinker, MachORpath, MachOSourceVersion, MachOUuid,
    MachOVersionMin,
};
use object::{macho, read, write, Architecture, BinaryFormat, Endianness};

// Test that segment size is valid when the first section needs alignment.
#[test]
fn issue_286_segment_file_size() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[1; 30], 0x1000);

    let bytes = &*object.write().unwrap();
    let header = macho::MachHeader64::parse(bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();
    let mut commands = header.load_commands(endian, bytes, 0).unwrap();
    let command = commands.next().unwrap().unwrap();
    let (segment, _) = command.segment_64().unwrap().unwrap();
    assert_eq!(segment.vmsize.get(endian), 30);
    assert_eq!(segment.filesize.get(endian), 30);
}

// We were emitting section file alignment padding that didn't match the address alignment padding.
#[test]
fn issue_552_section_file_alignment() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    // The starting file offset is not a multiple of 32 (checked later).
    // Length of 32 ensures that the file offset of the end of this section is still not a
    // multiple of 32.
    let section = object.add_section(vec![], vec![], object::SectionKind::ReadOnlyDataWithRel);
    object.append_section_data(section, &[0u8; 32], 1);

    // Address is already aligned correctly, so there must not any padding,
    // even though file offset is not aligned.
    let section = object.add_section(vec![], vec![], object::SectionKind::ReadOnlyData);
    object.append_section_data(section, &[0u8; 1], 32);

    let bytes = &*object.write().unwrap();
    //std::fs::write(&"align.o", &bytes).unwrap();
    let object = read::File::parse(bytes).unwrap();
    let mut sections = object.sections();

    let section = sections.next().unwrap();
    let offset = section.file_range().unwrap().0;
    // Check file offset is not aligned to 32.
    assert_ne!(offset % 32, 0);
    assert_eq!(section.address(), 0);
    assert_eq!(section.size(), 32);

    let section = sections.next().unwrap();
    // Check there is no padding.
    assert_eq!(section.file_range(), Some((offset + 32, 1)));
    assert_eq!(section.address(), 32);
    assert_eq!(section.size(), 1);
}

// Test creating a Mach-O executable with file type, LC_LOAD_DYLINKER, and LC_MAIN
#[test]
fn macho_executable_with_entry_point() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    // Set the file type to MH_EXECUTE
    object.set_macho_file_type(macho::MH_EXECUTE);

    // Add the dynamic linker path
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());

    // Set the entry point (file offset 0x1000 is typical for executables)
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));

    // Add a text section with some dummy code
    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0x90; 16], 1); // NOP instructions

    // Write and verify the object file
    let bytes = &*object.write().unwrap();

    // Parse the header to verify file type
    let header = macho::MachHeader64::parse(bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    // Verify the file type is MH_EXECUTE
    assert_eq!(header.filetype(endian), macho::MH_EXECUTE);

    // Parse and verify load commands
    let mut commands = header.load_commands(endian, bytes, 0).unwrap();

    let mut found_segment = false;
    let mut found_dylinker = false;
    let mut found_entry_point = false;
    let mut found_symtab = false;
    let mut found_dysymtab = false;

    loop {
        match commands.next() {
            Ok(Some(command)) => match command.variant().unwrap() {
                LoadCommandVariant::Segment64(_, _) => {
                    found_segment = true;
                }
                LoadCommandVariant::LoadDylinker(_) => {
                    found_dylinker = true;
                }
                LoadCommandVariant::EntryPoint(entry) => {
                    found_entry_point = true;
                    assert_eq!(entry.entryoff.get(endian), 0x1000);
                    assert_eq!(entry.stacksize.get(endian), 0);
                }
                LoadCommandVariant::Symtab(_) => {
                    found_symtab = true;
                }
                LoadCommandVariant::Dysymtab(_) => {
                    found_dysymtab = true;
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    // Verify all expected load commands are present
    assert!(found_segment, "LC_SEGMENT_64 not found");
    assert!(found_dylinker, "LC_LOAD_DYLINKER not found");
    assert!(found_entry_point, "LC_MAIN not found");
    assert!(found_symtab, "LC_SYMTAB not found");
    assert!(found_dysymtab, "LC_DYSYMTAB not found");
}

// Test creating a Mach-O dynamic library
#[test]
fn macho_dylib_file_type() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );

    // Set the file type to MH_DYLIB
    object.set_macho_file_type(macho::MH_DYLIB);

    // Add a text section
    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 8], 1);

    // Write and verify
    let bytes = &*object.write().unwrap();
    let header = macho::MachHeader64::parse(bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    // Verify the file type is MH_DYLIB
    assert_eq!(header.filetype(endian), macho::MH_DYLIB);
}

// Test creating a Mach-O executable with multiple RPATHs
#[test]
fn macho_executable_with_rpaths() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );

    // Set the file type to MH_EXECUTE
    object.set_macho_file_type(macho::MH_EXECUTE);

    // Add the dynamic linker
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());

    // Set the entry point
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));

    // Add multiple RPATHs (common patterns for macOS/iOS apps)
    object.add_macho_rpath(MachORpath::from_str("@executable_path/../Frameworks"));
    object.add_macho_rpath(MachORpath::from_str("@loader_path/Frameworks"));
    object.add_macho_rpath(MachORpath::from_str("/usr/local/lib"));

    // Add a text section
    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    // Write and verify
    let bytes = &*object.write().unwrap();
    let header = macho::MachHeader64::parse(bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    // Verify file type
    assert_eq!(header.filetype(endian), macho::MH_EXECUTE);

    // Parse and count RPATHs
    let mut commands = header.load_commands(endian, bytes, 0).unwrap();
    let mut rpath_count = 0;
    let mut found_rpaths = Vec::new();

    loop {
        match commands.next() {
            Ok(Some(command)) => {
                if let LoadCommandVariant::Rpath(_) = command.variant().unwrap() {
                    rpath_count += 1;
                    // We could verify the actual path here if we implement string reading
                    found_rpaths.push(command.cmd());
                }
            }
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    // Verify we have all 3 RPATHs
    assert_eq!(rpath_count, 3, "Expected 3 LC_RPATH commands");
    for cmd in &found_rpaths {
        assert_eq!(*cmd, macho::LC_RPATH);
    }
}

// Test creating a Mach-O dylib with ID and dependencies
#[test]
fn macho_dylib_with_dependencies() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );

    // Set the file type to MH_DYLIB
    object.set_macho_file_type(macho::MH_DYLIB);

    // Set the dylib ID (install name)
    let mut id_dylib = MachODylib::from_str("@rpath/MyLibrary.dylib");
    id_dylib.current_version = MachODylib::encode_version(2, 5, 1); // 2.5.1
    id_dylib.compatibility_version = MachODylib::encode_version(1, 0, 0); // 1.0.0
    object.set_macho_id_dylib(id_dylib);

    // Add library dependencies
    object.add_macho_load_dylib(MachODylib::from_str("/usr/lib/libSystem.B.dylib"));
    object.add_macho_load_dylib(MachODylib::from_str(
        "@rpath/OtherFramework.framework/OtherFramework",
    ));

    // Add an rpath
    object.add_macho_rpath(MachORpath::from_str("@loader_path"));

    // Add a text section
    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    // Write and verify
    let bytes = &*object.write().unwrap();
    let header = macho::MachHeader64::parse(bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    // Verify file type
    assert_eq!(header.filetype(endian), macho::MH_DYLIB);

    // Parse and verify load commands
    let mut commands = header.load_commands(endian, bytes, 0).unwrap();
    let mut found_id_dylib = false;
    let mut load_dylib_count = 0;
    let mut found_rpath = false;

    loop {
        match commands.next() {
            Ok(Some(command)) => match command.variant().unwrap() {
                LoadCommandVariant::IdDylib(dylib_cmd) => {
                    found_id_dylib = true;
                    // Verify version info
                    assert_eq!(
                        dylib_cmd.dylib.current_version.get(endian),
                        MachODylib::encode_version(2, 5, 1)
                    );
                    assert_eq!(
                        dylib_cmd.dylib.compatibility_version.get(endian),
                        MachODylib::encode_version(1, 0, 0)
                    );
                }
                LoadCommandVariant::Dylib(_) => {
                    load_dylib_count += 1;
                }
                LoadCommandVariant::Rpath(_) => {
                    found_rpath = true;
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    // Verify all expected load commands are present
    assert!(found_id_dylib, "LC_ID_DYLIB not found");
    assert_eq!(load_dylib_count, 2, "Expected 2 LC_LOAD_DYLIB commands");
    assert!(found_rpath, "LC_RPATH not found");
}
