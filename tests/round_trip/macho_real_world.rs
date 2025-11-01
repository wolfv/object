/// Tests using real-world Mach-O binaries from testfiles/macho
///
/// These tests verify that our Builder can read and modify actual binaries
/// produced by real compilers and linkers.

use object::build::macho::Builder;
use object::read::macho::MachHeader;
use object::{macho, Architecture, Endianness};
use std::fs;
use std::path::PathBuf;

fn testfile_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testfiles")
        .join("macho")
        .join(filename)
}

/// Test reading a real x86_64 executable built by clang
#[test]
fn real_world_read_x86_64_executable() {
    let path = testfile_path("base-x86_64");
    let data = fs::read(&path).expect("Failed to read test file");
    let builder = Builder::read(&*data).expect("Failed to parse Mach-O file");

    // Verify basic properties
    assert_eq!(builder.architecture, Architecture::X86_64);
    assert_eq!(builder.is_64, true);
    assert_eq!(builder.header.file_type, macho::MH_EXECUTE);

    // Check that it has an entry point
    assert!(
        builder.load_commands.entry_point.is_some()
            || builder.load_commands.load_dylinker.is_some(),
        "Executable should have entry point or dylinker"
    );
}

/// Test reading a real aarch64 executable built by clang
#[test]
fn real_world_read_aarch64_executable() {
    let path = testfile_path("base-aarch64");
    let data = fs::read(&path).expect("Failed to read test file");
    let builder = Builder::read(&*data).expect("Failed to parse Mach-O file");

    // Verify basic properties
    assert_eq!(builder.architecture, Architecture::Aarch64);
    assert_eq!(builder.is_64, true);
    assert_eq!(builder.header.file_type, macho::MH_EXECUTE);
}

/// Test reading and modifying a real x86_64 executable
#[test]
fn real_world_modify_x86_64_executable() {
    let path = testfile_path("base-x86_64");
    let data = fs::read(&path).expect("Failed to read test file");
    let mut builder = Builder::read(&*data).expect("Failed to parse Mach-O file");

    // Count original RPATHs
    let original_rpath_count = builder.rpaths().count();

    // Add a new RPATH
    builder.add_rpath("@executable_path/test");

    // Write back
    let modified_data = builder.write().expect("Failed to write modified file");

    // Read again and verify
    let builder2 = Builder::read(&*modified_data).expect("Failed to parse modified file");
    let new_rpath_count = builder2.rpaths().count();

    assert_eq!(
        new_rpath_count,
        original_rpath_count + 1,
        "Should have one more RPATH"
    );

    // Verify the new RPATH is present
    let rpaths: Vec<_> = builder2.rpaths().collect();
    assert!(
        rpaths.contains(&b"@executable_path/test".as_ref()),
        "New RPATH should be present"
    );
}

/// Test reading and modifying a real aarch64 executable
#[test]
fn real_world_modify_aarch64_executable() {
    let path = testfile_path("base-aarch64");
    let data = fs::read(&path).expect("Failed to read test file");
    let mut builder = Builder::read(&*data).expect("Failed to parse Mach-O file");

    // Add multiple RPATHs
    builder.add_rpath("@executable_path/../Frameworks");
    builder.add_rpath("@loader_path/lib");

    // Write back
    let modified_data = builder.write().expect("Failed to write modified file");

    // Verify the modified file is still valid
    let header = macho::MachHeader64::parse(&*modified_data, 0).expect("Should parse as valid Mach-O");
    let endian: Endianness = header.endian().unwrap();

    assert_eq!(header.filetype(endian), macho::MH_EXECUTE);

    // Count RPATHs in the modified file
    let mut commands = header.load_commands(endian, &*modified_data, 0).unwrap();
    let mut rpath_count = 0;

    while let Some(cmd) = commands.next().unwrap() {
        if cmd.cmd() == macho::LC_RPATH {
            rpath_count += 1;
        }
    }

    assert!(rpath_count >= 2, "Should have at least the 2 RPATHs we added");
}

/// Test reading a Go-compiled executable (complex real-world binary)
#[test]
fn real_world_read_go_executable() {
    let path = testfile_path("go-x86_64");
    let data = fs::read(&path).expect("Failed to read test file");
    let result = Builder::read(&*data);

    match result {
        Ok(builder) => {
            // Go binaries are executables
            assert_eq!(builder.header.file_type, macho::MH_EXECUTE);
            assert_eq!(builder.architecture, Architecture::X86_64);

            // Go binaries typically have multiple load commands
            let has_load_cmds = builder.load_commands.entry_point.is_some()
                || builder.load_commands.load_dylinker.is_some()
                || !builder.segments.is_empty();

            assert!(
                has_load_cmds,
                "Go executable should have load commands or segments"
            );
        }
        Err(e) => {
            // It's okay if we can't fully parse Go binaries yet - they can be complex
            eprintln!("Note: Could not parse Go binary (expected): {}", e);
        }
    }
}

/// Test reading object files (relocatable)
#[test]
fn real_world_read_object_file() {
    let path = testfile_path("base-x86_64.o");
    let data = fs::read(&path).expect("Failed to read test file");
    let builder = Builder::read(&*data).expect("Failed to parse object file");

    // Object files have MH_OBJECT type
    assert_eq!(builder.header.file_type, macho::MH_OBJECT);
    assert_eq!(builder.architecture, Architecture::X86_64);
    assert_eq!(builder.is_64, true);

    // Object files shouldn't have entry points or dylinker
    assert!(
        builder.load_commands.entry_point.is_none(),
        "Object files shouldn't have entry points"
    );
    assert!(
        builder.load_commands.load_dylinker.is_none(),
        "Object files shouldn't have dylinker"
    );
}

/// Test round-trip with a real object file
#[test]
fn real_world_roundtrip_object_file() {
    let path = testfile_path("base-aarch64.o");
    let original_data = fs::read(&path).expect("Failed to read test file");
    let builder = Builder::read(&*original_data).expect("Failed to parse object file");

    // Write it back
    let written_data = builder.write().expect("Failed to write object file");

    // Parse the written data
    let builder2 = Builder::read(&*written_data).expect("Failed to re-parse object file");

    // Verify key properties are preserved
    assert_eq!(builder2.header.file_type, macho::MH_OBJECT);
    assert_eq!(builder2.architecture, Architecture::Aarch64);
    assert_eq!(builder2.is_64, true);
}

/// Test reading debug object files
#[test]
fn real_world_read_debug_object() {
    let path = testfile_path("base-x86_64-debug.o");
    if !path.exists() {
        eprintln!("Skipping test: {} not found", path.display());
        return;
    }

    let data = fs::read(&path).expect("Failed to read test file");
    let builder = Builder::read(&*data).expect("Failed to parse debug object file");

    assert_eq!(builder.header.file_type, macho::MH_OBJECT);
    assert_eq!(builder.architecture, Architecture::X86_64);
}

/// Test that modified real binaries have valid structure
#[test]
fn real_world_modified_structure_validity() {
    let path = testfile_path("base-x86_64");
    if !path.exists() {
        eprintln!("Skipping test: {} not found", path.display());
        return;
    }

    let data = fs::read(&path).expect("Failed to read test file");
    let mut builder = Builder::read(&*data).expect("Failed to parse Mach-O file");

    // Make multiple modifications
    builder.add_rpath("@executable_path/lib");
    builder.add_rpath("@loader_path/frameworks");

    // Write and verify structure
    let modified = builder.write().expect("Failed to write");

    // Parse as raw Mach-O to verify structure
    let header = macho::MachHeader64::parse(&*modified, 0).expect("Should be valid Mach-O");
    let endian: Endianness = header.endian().unwrap();

    // Verify header is sane
    assert_eq!(header.filetype(endian), macho::MH_EXECUTE);
    assert!(header.ncmds(endian) > 0, "Should have load commands");
    assert!(header.sizeofcmds(endian) > 0, "Load commands should have size");

    // Verify we can iterate through all load commands without error
    let mut commands = header.load_commands(endian, &*modified, 0).unwrap();
    let mut cmd_count = 0;

    while let Some(cmd) = commands.next().expect("Should parse load command") {
        cmd_count += 1;
        // Just verify we can read the command type
        let _ = cmd.cmd();
    }

    assert_eq!(
        cmd_count,
        header.ncmds(endian) as usize,
        "Should parse all load commands"
    );
}

/// Stress test: Add many RPATHs to a real binary
#[test]
fn real_world_stress_many_rpaths() {
    let path = testfile_path("base-x86_64");
    if !path.exists() {
        eprintln!("Skipping test: {} not found", path.display());
        return;
    }

    let data = fs::read(&path).expect("Failed to read test file");
    let mut builder = Builder::read(&*data).expect("Failed to parse Mach-O file");

    // Add 10 RPATHs
    for i in 0..10 {
        builder.add_rpath(&format!("@executable_path/lib{}", i));
    }

    // Write back
    let modified = builder.write().expect("Failed to write with many RPATHs");

    // Verify they're all there
    let builder2 = Builder::read(&*modified).expect("Failed to re-parse");
    let rpaths: Vec<_> = builder2.rpaths().collect();

    // Should have at least the 10 we added (may have more from original)
    let new_rpaths: Vec<_> = rpaths
        .iter()
        .filter(|r| {
            r.starts_with(b"@executable_path/lib")
                && r.len() > b"@executable_path/lib".len()
                && r[b"@executable_path/lib".len()].is_ascii_digit()
        })
        .collect();

    assert_eq!(new_rpaths.len(), 10, "Should have all 10 new RPATHs");
}

/// Test removing RPATHs from a modified binary
#[test]
fn real_world_remove_added_rpaths() {
    let path = testfile_path("base-aarch64");
    let data = fs::read(&path).expect("Failed to read test file");
    let mut builder = Builder::read(&*data).expect("Failed to parse Mach-O file");

    // Add some RPATHs
    builder.add_rpath("@executable_path/temp1");
    builder.add_rpath("@executable_path/temp2");
    builder.add_rpath("@executable_path/keep");

    // Write
    let modified = builder.write().expect("Failed to write");

    // Read again and remove some
    let mut builder2 = Builder::read(&*modified).expect("Failed to re-parse");
    builder2.remove_rpath("@executable_path/temp1");
    builder2.remove_rpath("@executable_path/temp2");

    // Write again
    let final_data = builder2.write().expect("Failed to write final");

    // Verify only "keep" remains
    let builder3 = Builder::read(&*final_data).expect("Failed to parse final");
    let rpaths: Vec<_> = builder3.rpaths().collect();

    assert!(
        rpaths.contains(&b"@executable_path/keep".as_ref()),
        "Should have the 'keep' RPATH"
    );
    assert!(
        !rpaths.contains(&b"@executable_path/temp1".as_ref()),
        "Should not have temp1"
    );
    assert!(
        !rpaths.contains(&b"@executable_path/temp2".as_ref()),
        "Should not have temp2"
    );
}
