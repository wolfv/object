//! A simple objcopy-like tool for Mach-O binaries.
//!
//! This tool reads a Mach-O binary and writes it back out, testing
//! whether we can achieve bit-for-bit identical copies.
//!
//! **CURRENT LIMITATION**: This implementation can parse and modify load commands
//! but does NOT preserve executable code/data segments. Therefore, it cannot
//! produce bit-for-bit identical copies of complete executables or libraries.
//!
//! It CAN successfully preserve and modify:
//! - Load commands (LC_RPATH, LC_LOAD_DYLIB, LC_ID_DYLIB, etc.)
//! - Fat/universal binary structure
//! - File metadata
//!
//! For actual binary copying with segment preservation, use the native tools
//! (install_name_tool, lipo, etc.) or wait for full segment support.
//!
//! Usage:
//!   macho-objcopy <input> <output>
//!   macho-objcopy --verify <input>     # Read and write to temp, then compare

use object::build::macho::Builder;
use object::build::macho_fat::FatBuilder;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

fn print_usage() {
    eprintln!(
        r#"macho-objcopy - Copy Mach-O binaries (testing bit-for-bit preservation)

NOTE: Current implementation does NOT preserve executable segments, so it cannot
      produce bit-for-bit identical copies. This tool is for testing purposes only.

Usage:
  macho-objcopy <input> <output>      Copy input to output (load commands only)
  macho-objcopy --verify <input>      Verify round-trip (will show differences)
  macho-objcopy -h, --help            Show this help message

Examples:
  # Attempt to copy a binary (will show size difference)
  macho-objcopy myapp myapp-copy

  # Verify round-trip and see what's preserved
  macho-objcopy --verify myapp
"#
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Error: No input file specified");
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "-h" | "--help" => {
            print_usage();
            process::exit(0);
        }
        "--verify" => {
            if args.len() < 3 {
                eprintln!("Error: --verify requires an input file");
                print_usage();
                process::exit(1);
            }
            verify_roundtrip(&PathBuf::from(&args[2]));
        }
        _ => {
            if args.len() < 3 {
                eprintln!("Error: Output file not specified");
                print_usage();
                process::exit(1);
            }
            copy_binary(&PathBuf::from(&args[1]), &PathBuf::from(&args[2]));
        }
    }
}

fn copy_binary(input: &PathBuf, output: &PathBuf) {
    println!("Copying {} to {}", input.display(), output.display());

    // Read input file
    let data = match fs::read(input) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading {}: {}", input.display(), e);
            process::exit(1);
        }
    };

    // Try to parse as fat binary first
    let output_data = if let Ok(fat_builder) = FatBuilder::read(&*data) {
        println!("Detected universal binary with {} architectures", fat_builder.len());
        match fat_builder.write() {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Error writing fat Mach-O file: {}", e);
                process::exit(1);
            }
        }
    } else {
        // Parse as single-architecture Mach-O file
        let builder = match Builder::read(&*data) {
            Ok(builder) => builder,
            Err(e) => {
                eprintln!("Error parsing Mach-O file {}: {}", input.display(), e);
                process::exit(1);
            }
        };

        println!("Detected single-architecture Mach-O binary");
        match builder.write() {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Error writing Mach-O file: {}", e);
                process::exit(1);
            }
        }
    };

    // Write output file
    if let Err(e) = fs::write(output, &output_data) {
        eprintln!("Error writing to {}: {}", output.display(), e);
        process::exit(1);
    }

    println!("Successfully wrote: {}", output.display());

    // Compare sizes
    let input_size = data.len();
    let output_size = output_data.len();
    println!("\nSize comparison:");
    println!("  Input:  {} bytes", input_size);
    println!("  Output: {} bytes", output_size);

    if input_size == output_size {
        println!("  ✓ Sizes match");
    } else {
        println!("  ✗ Size mismatch (diff: {} bytes)",
                 (output_size as i64 - input_size as i64).abs());
    }

    // Check if identical
    if data == output_data {
        println!("\n✓ Files are bit-for-bit identical!");
        process::exit(0);
    } else {
        println!("\n✗ Files differ");

        // Find first difference
        for (i, (a, b)) in data.iter().zip(output_data.iter()).enumerate() {
            if a != b {
                println!("  First difference at offset 0x{:x}: 0x{:02x} -> 0x{:02x}", i, a, b);
                break;
            }
        }

        process::exit(1);
    }
}

fn verify_roundtrip(input: &PathBuf) {
    println!("Verifying round-trip for {}", input.display());

    // Read input file
    let original_data = match fs::read(input) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading {}: {}", input.display(), e);
            process::exit(1);
        }
    };

    println!("Original file size: {} bytes", original_data.len());

    // Try to parse as fat binary first
    let output_data = if let Ok(fat_builder) = FatBuilder::read(&*original_data) {
        println!("Detected universal binary with {} architectures", fat_builder.len());

        // Show architecture info
        for (i, slice) in fat_builder.iter().enumerate() {
            println!("  Architecture {}: CPU type 0x{:x}, subtype 0x{:x}",
                     i, slice.cpu_type(), slice.cpu_subtype());
        }

        match fat_builder.write() {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Error writing fat Mach-O file: {}", e);
                process::exit(1);
            }
        }
    } else {
        // Parse as single-architecture Mach-O file
        let builder = match Builder::read(&*original_data) {
            Ok(builder) => builder,
            Err(e) => {
                eprintln!("Error parsing Mach-O file {}: {}", input.display(), e);
                process::exit(1);
            }
        };

        println!("Detected single-architecture Mach-O binary");
        println!("  CPU type: 0x{:x}, subtype: 0x{:x}",
                 builder.cpu_type(), builder.cpu_subtype());

        match builder.write() {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Error writing Mach-O file: {}", e);
                process::exit(1);
            }
        }
    };

    println!("\nRound-trip complete");
    println!("  Original: {} bytes", original_data.len());
    println!("  Output:   {} bytes", output_data.len());

    if original_data.len() == output_data.len() {
        println!("  ✓ Sizes match");
    } else {
        println!("  ✗ Size mismatch (diff: {} bytes)",
                 (output_data.len() as i64 - original_data.len() as i64).abs());
    }

    // Check if identical
    if original_data == output_data {
        println!("\n✓ PASS: Round-trip produces bit-for-bit identical binary!");
        process::exit(0);
    } else {
        println!("\n✗ FAIL: Round-trip produces different binary");

        // Find first difference
        let min_len = original_data.len().min(output_data.len());
        let mut first_diff = None;
        for i in 0..min_len {
            if original_data[i] != output_data[i] {
                first_diff = Some(i);
                break;
            }
        }

        if let Some(offset) = first_diff {
            println!("  First difference at offset 0x{:x} (byte {})", offset, offset);
            println!("    Original: 0x{:02x}", original_data[offset]);
            println!("    Output:   0x{:02x}", output_data[offset]);

            // Show context (16 bytes before and after)
            let start = offset.saturating_sub(16);
            let end = (offset + 16).min(min_len);

            println!("\n  Context (offset 0x{:x} to 0x{:x}):", start, end);
            print!("    Original: ");
            for i in start..end {
                if i == offset {
                    print!("[{:02x}] ", original_data[i]);
                } else {
                    print!("{:02x} ", original_data[i]);
                }
            }
            println!();

            print!("    Output:   ");
            for i in start..end {
                if i == offset {
                    print!("[{:02x}] ", output_data[i]);
                } else {
                    print!("{:02x} ", output_data[i]);
                }
            }
            println!();
        } else if original_data.len() != output_data.len() {
            println!("  Files are identical up to {} bytes, but have different lengths", min_len);
        }

        process::exit(1);
    }
}
