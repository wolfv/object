/// Tests for extended Mach-O load commands (LC_UUID, LC_SOURCE_VERSION, LC_VERSION_MIN_*, etc.)

use object::read::macho::{LoadCommandVariant, MachHeader};
use object::write::{
    MachODylib, MachOEntryPoint, MachOLoadDylinker, MachOSourceVersion, MachOUuid,
    MachOVersionMin,
};
use object::{macho, write, Architecture, BinaryFormat, Endianness};

/// Test LC_UUID command
#[test]
fn macho_uuid_command() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_EXECUTE);
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));

    // Set a specific UUID
    let test_uuid = [
        0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54,
        0x32, 0x10,
    ];
    object.set_macho_uuid(MachOUuid::new(test_uuid));

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0x90; 16], 1);

    let bytes = object.write().unwrap();

    // Parse and verify
    let header = macho::MachHeader64::parse(&*bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    let mut commands = header.load_commands(endian, &*bytes, 0).unwrap();
    let mut found_uuid = false;

    loop {
        match commands.next() {
            Ok(Some(command)) => {
                if let LoadCommandVariant::Uuid(uuid_cmd) = command.variant().unwrap() {
                    found_uuid = true;
                    assert_eq!(uuid_cmd.uuid, test_uuid, "UUID should match");
                }
            }
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    assert!(found_uuid, "LC_UUID not found");
}

/// Test LC_SOURCE_VERSION command
#[test]
fn macho_source_version_command() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_DYLIB);
    object.set_macho_id_dylib(MachODylib::from_str("@rpath/MyLib.dylib"));

    // Set source version 1.2.3.4.5
    let source_version = MachOSourceVersion::encode_version(1, 2, 3, 4, 5);
    object.set_macho_source_version(MachOSourceVersion::new(source_version));

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    let bytes = object.write().unwrap();

    // Parse and verify
    let header = macho::MachHeader64::parse(&*bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    let mut commands = header.load_commands(endian, &*bytes, 0).unwrap();
    let mut found_source_version = false;

    loop {
        match commands.next() {
            Ok(Some(command)) => {
                if let LoadCommandVariant::SourceVersion(sv_cmd) = command.variant().unwrap() {
                    found_source_version = true;
                    assert_eq!(
                        sv_cmd.version.get(endian),
                        source_version,
                        "Source version should match"
                    );
                }
            }
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    assert!(found_source_version, "LC_SOURCE_VERSION not found");
}

/// Test LC_VERSION_MIN_MACOSX command
#[test]
fn macho_version_min_macosx() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_EXECUTE);
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));

    // Set minimum macOS version to 10.15.0, SDK 11.0.0
    let min_version = MachOVersionMin::new(
        MachOVersionMin::encode_version(10, 15, 0),
        MachOVersionMin::encode_version(11, 0, 0),
    );
    object.set_macho_version_min_macosx(min_version);

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0x90; 16], 1);

    let bytes = object.write().unwrap();

    // Parse and verify
    let header = macho::MachHeader64::parse(&*bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    let mut commands = header.load_commands(endian, &*bytes, 0).unwrap();
    let mut found_version_min = false;

    loop {
        match commands.next() {
            Ok(Some(command)) => match command.variant().unwrap() {
                LoadCommandVariant::VersionMin(vm_cmd) => {
                    if command.cmd() == macho::LC_VERSION_MIN_MACOSX {
                        found_version_min = true;
                        assert_eq!(
                            vm_cmd.version.get(endian),
                            MachOVersionMin::encode_version(10, 15, 0),
                            "Version should be 10.15.0"
                        );
                        assert_eq!(
                            vm_cmd.sdk.get(endian),
                            MachOVersionMin::encode_version(11, 0, 0),
                            "SDK should be 11.0.0"
                        );
                    }
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    assert!(found_version_min, "LC_VERSION_MIN_MACOSX not found");
}

/// Test LC_VERSION_MIN_IPHONEOS command
#[test]
fn macho_version_min_iphoneos() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_EXECUTE);
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));

    // Set minimum iOS version to 14.0.0, SDK 15.0.0
    let min_version = MachOVersionMin::new(
        MachOVersionMin::encode_version(14, 0, 0),
        MachOVersionMin::encode_version(15, 0, 0),
    );
    object.set_macho_version_min_iphoneos(min_version);

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    let bytes = object.write().unwrap();

    // Parse and verify
    let header = macho::MachHeader64::parse(&*bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    let mut commands = header.load_commands(endian, &*bytes, 0).unwrap();
    let mut found_version_min = false;

    loop {
        match commands.next() {
            Ok(Some(command)) => match command.variant().unwrap() {
                LoadCommandVariant::VersionMin(vm_cmd) => {
                    if command.cmd() == macho::LC_VERSION_MIN_IPHONEOS {
                        found_version_min = true;
                        assert_eq!(
                            vm_cmd.version.get(endian),
                            MachOVersionMin::encode_version(14, 0, 0),
                            "Version should be 14.0.0"
                        );
                        assert_eq!(
                            vm_cmd.sdk.get(endian),
                            MachOVersionMin::encode_version(15, 0, 0),
                            "SDK should be 15.0.0"
                        );
                    }
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    assert!(found_version_min, "LC_VERSION_MIN_IPHONEOS not found");
}

/// Test multiple version commands together
#[test]
fn macho_multiple_metadata_commands() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_DYLIB);
    object.set_macho_id_dylib(MachODylib::from_str("@rpath/MyFramework.dylib"));

    // Add UUID
    let test_uuid = [0xAA; 16];
    object.set_macho_uuid(MachOUuid::new(test_uuid));

    // Add source version
    let source_version = MachOSourceVersion::encode_version(2, 0, 1, 0, 0);
    object.set_macho_source_version(MachOSourceVersion::new(source_version));

    // Add version min
    let min_version = MachOVersionMin::new(
        MachOVersionMin::encode_version(10, 14, 0),
        MachOVersionMin::encode_version(10, 15, 0),
    );
    object.set_macho_version_min_macosx(min_version);

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0; 16], 1);

    let bytes = object.write().unwrap();

    // Parse and verify all commands are present
    let header = macho::MachHeader64::parse(&*bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    let mut commands = header.load_commands(endian, &*bytes, 0).unwrap();
    let mut found_uuid = false;
    let mut found_source_version = false;
    let mut found_version_min = false;

    loop {
        match commands.next() {
            Ok(Some(command)) => match command.variant().unwrap() {
                LoadCommandVariant::Uuid(uuid_cmd) => {
                    found_uuid = true;
                    assert_eq!(uuid_cmd.uuid, test_uuid);
                }
                LoadCommandVariant::SourceVersion(sv_cmd) => {
                    found_source_version = true;
                    assert_eq!(sv_cmd.version.get(endian), source_version);
                }
                LoadCommandVariant::VersionMin(vm_cmd) => {
                    if command.cmd() == macho::LC_VERSION_MIN_MACOSX {
                        found_version_min = true;
                    }
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    assert!(found_uuid, "LC_UUID not found");
    assert!(found_source_version, "LC_SOURCE_VERSION not found");
    assert!(found_version_min, "LC_VERSION_MIN_MACOSX not found");
}

/// Test random UUID generation (std feature only)
#[test]
#[cfg(feature = "std")]
fn macho_random_uuid() {
    let mut object = write::Object::new(
        BinaryFormat::MachO,
        Architecture::X86_64,
        Endianness::Little,
    );

    object.set_macho_file_type(macho::MH_EXECUTE);
    object.set_macho_load_dylinker(MachOLoadDylinker::default_dyld());
    object.set_macho_entry_point(MachOEntryPoint::new(0x1000));

    // Generate a random UUID
    let uuid = MachOUuid::random();
    object.set_macho_uuid(uuid);

    let text = object.section_id(write::StandardSection::Text);
    object.append_section_data(text, &[0x90; 16], 1);

    let bytes = object.write().unwrap();

    // Parse and verify UUID is present and valid
    let header = macho::MachHeader64::parse(&*bytes, 0).unwrap();
    let endian: Endianness = header.endian().unwrap();

    let mut commands = header.load_commands(endian, &*bytes, 0).unwrap();
    let mut found_uuid = false;

    loop {
        match commands.next() {
            Ok(Some(command)) => {
                if let LoadCommandVariant::Uuid(uuid_cmd) = command.variant().unwrap() {
                    found_uuid = true;
                    // Verify it's a valid UUID v4 (version bits are set correctly)
                    assert_eq!(uuid_cmd.uuid[6] & 0xF0, 0x40, "UUID should be version 4");
                    assert_eq!(
                        uuid_cmd.uuid[8] & 0xC0,
                        0x80,
                        "UUID should have RFC 4122 variant"
                    );
                }
            }
            Ok(None) => break,
            Err(e) => panic!("Failed to read load command: {}", e),
        }
    }

    assert!(found_uuid, "LC_UUID not found");
}
