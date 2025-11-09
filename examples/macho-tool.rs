//! A command-line tool for manipulating Mach-O binaries, similar to install_name_tool.
//!
//! Usage:
//!   macho-tool [OPTIONS] <file>
//!
//! Options:
//!   -add_rpath <path>           Add an rpath search path
//!   -delete_rpath <path>        Delete an rpath search path
//!   -rpath <old> <new>          Change an rpath path from old to new
//!   -change <old> <new>         Change dependent dylib install name from old to new
//!   -id <name>                  Change dylib identification name
//!   -o <output>                 Write output to a different file (default: modify in-place)
//!   --backup                    Create backup with .bak extension before modifying
//!   --list                      List current install names and rpaths (read-only)

use object::build::macho::Builder;
use object::build::macho_fat::FatBuilder;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

struct Options {
    input_file: PathBuf,
    output_file: Option<PathBuf>,
    backup: bool,
    list_only: bool,
    add_rpaths: Vec<String>,
    delete_rpaths: Vec<String>,
    change_rpaths: Vec<(String, String)>,
    change_dylibs: Vec<(String, String)>,
    new_id: Option<String>,
}

fn print_usage() {
    eprintln!(
        r#"macho-tool - Manipulate Mach-O binaries (Rust implementation)

Usage:
  macho-tool [OPTIONS] <file>

Options:
  -add_rpath <path>           Add an rpath search path
  -delete_rpath <path>        Delete an rpath search path
  -rpath <old> <new>          Change an rpath path from old to new
  -change <old> <new>         Change dependent dylib install name from old to new
  -id <name>                  Change dylib identification name
  -o <output>                 Write output to a different file (default: modify in-place)
  --backup                    Create backup with .bak extension before modifying
  --list                      List current install names and rpaths (read-only)
  -h, --help                  Show this help message

Examples:
  # Add an rpath
  macho-tool -add_rpath @executable_path/../Frameworks myapp

  # Change dylib dependency
  macho-tool -change /usr/lib/old.dylib /usr/lib/new.dylib myapp

  # Change dylib ID
  macho-tool -id @rpath/MyFramework.framework/MyFramework MyFramework

  # List all install names and rpaths
  macho-tool --list myapp
"#
    );
}

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        return Err("No input file specified".to_string());
    }

    let mut opts = Options {
        input_file: PathBuf::new(),
        output_file: None,
        backup: false,
        list_only: false,
        add_rpaths: Vec::new(),
        delete_rpaths: Vec::new(),
        change_rpaths: Vec::new(),
        change_dylibs: Vec::new(),
        new_id: None,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "--list" => {
                opts.list_only = true;
                i += 1;
            }
            "--backup" => {
                opts.backup = true;
                i += 1;
            }
            "-o" => {
                if i + 1 >= args.len() {
                    return Err("-o requires an argument".to_string());
                }
                opts.output_file = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "-add_rpath" => {
                if i + 1 >= args.len() {
                    return Err("-add_rpath requires an argument".to_string());
                }
                opts.add_rpaths.push(args[i + 1].clone());
                i += 2;
            }
            "-delete_rpath" => {
                if i + 1 >= args.len() {
                    return Err("-delete_rpath requires an argument".to_string());
                }
                opts.delete_rpaths.push(args[i + 1].clone());
                i += 2;
            }
            "-rpath" => {
                if i + 2 >= args.len() {
                    return Err("-rpath requires two arguments (old new)".to_string());
                }
                opts.change_rpaths
                    .push((args[i + 1].clone(), args[i + 2].clone()));
                i += 3;
            }
            "-change" => {
                if i + 2 >= args.len() {
                    return Err("-change requires two arguments (old new)".to_string());
                }
                opts.change_dylibs
                    .push((args[i + 1].clone(), args[i + 2].clone()));
                i += 3;
            }
            "-id" => {
                if i + 1 >= args.len() {
                    return Err("-id requires an argument".to_string());
                }
                opts.new_id = Some(args[i + 1].clone());
                i += 2;
            }
            arg if !arg.starts_with('-') => {
                if opts.input_file.as_os_str().is_empty() {
                    opts.input_file = PathBuf::from(arg);
                } else {
                    return Err(format!("Unexpected argument: {}", arg));
                }
                i += 1;
            }
            arg => {
                return Err(format!("Unknown option: {}", arg));
            }
        }
    }

    if opts.input_file.as_os_str().is_empty() {
        return Err("No input file specified".to_string());
    }

    Ok(opts)
}

fn list_info(builder: &Builder) {
    println!("Install Name:");
    if let Some(install_name) = builder.install_name() {
        println!("  {}", String::from_utf8_lossy(install_name));
    } else {
        println!("  (none - not a dylib)");
    }

    println!("\nRPaths:");
    let rpaths: Vec<_> = builder.rpaths().collect();
    if rpaths.is_empty() {
        println!("  (none)");
    } else {
        for rpath in rpaths {
            println!("  {}", String::from_utf8_lossy(rpath));
        }
    }

    println!("\nDependencies:");
    let deps: Vec<_> = builder.dependencies().collect();
    if deps.is_empty() {
        println!("  (none)");
    } else {
        for dep in deps {
            println!("  {}", String::from_utf8_lossy(dep));
        }
    }
}

fn main() {
    let opts = match parse_args() {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!();
            print_usage();
            process::exit(1);
        }
    };

    // Read input file
    let data = match fs::read(&opts.input_file) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading {}: {}", opts.input_file.display(), e);
            process::exit(1);
        }
    };

    // Try to parse as fat binary first
    match FatBuilder::read(&*data) {
        Ok(fat_builder) => {
            // Handle fat binary
            return handle_fat_binary(fat_builder, opts);
        }
        Err(e) => {
            eprintln!("Debug: Not a fat binary: {}", e);
        }
    }

    // Parse as single-architecture Mach-O file
    let mut builder = match Builder::read(&*data) {
        Ok(builder) => builder,
        Err(e) => {
            eprintln!(
                "Error parsing Mach-O file {}: {}",
                opts.input_file.display(),
                e
            );
            process::exit(1);
        }
    };

    handle_single_arch(builder, opts)
}

fn handle_fat_binary(mut fat_builder: FatBuilder, opts: Options) -> ! {
    println!("Detected universal binary with {} architectures", fat_builder.len());

    // Apply modifications to all slices
    let mut modified = false;

    for rpath in &opts.add_rpaths {
        println!("Adding rpath to all architectures: {}", rpath);
        fat_builder.for_each_slice(|builder| {
            builder.add_rpath(rpath);
        });
        modified = true;
    }

    for rpath in &opts.delete_rpaths {
        println!("Deleting rpath from all architectures: {}", rpath);
        fat_builder.for_each_slice(|builder| {
            builder.remove_rpath(rpath);
        });
        modified = true;
    }

    for (old, new) in &opts.change_rpaths {
        println!("Changing rpath in all architectures from '{}' to '{}'", old, new);
        fat_builder.for_each_slice(|builder| {
            builder.remove_rpath(old);
            builder.add_rpath(new);
        });
        modified = true;
    }

    for (old, new) in &opts.change_dylibs {
        println!("Changing dependency in all architectures from '{}' to '{}'", old, new);
        fat_builder.for_each_slice(|builder| {
            builder.change_dependency(old, new);
        });
        modified = true;
    }

    if let Some(new_id) = &opts.new_id {
        println!("Changing dylib ID in all architectures to: {}", new_id);
        fat_builder.for_each_slice(|builder| {
            builder.set_install_name(new_id, 0x10000, 0x10000);
        });
        modified = true;
    }

    // If list mode, show info
    if opts.list_only {
        for (i, slice) in fat_builder.iter().enumerate() {
            println!("\nArchitecture {}:", i);
            list_info(slice);
        }

        if !modified {
            process::exit(0);
        }
    }

    if !modified {
        eprintln!("Warning: No modifications specified");
        process::exit(0);
    }

    // Write output
    let output_data = match fat_builder.write() {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error writing fat Mach-O file: {}", e);
            process::exit(1);
        }
    };

    let output_path = opts.output_file.as_ref().unwrap_or(&opts.input_file);

    // Create backup if requested
    if opts.backup && output_path == &opts.input_file {
        let backup_path = {
            let mut p = opts.input_file.clone();
            let mut filename = p.file_name().unwrap().to_os_string();
            filename.push(".bak");
            p.set_file_name(filename);
            p
        };
        if let Err(e) = fs::copy(&opts.input_file, &backup_path) {
            eprintln!(
                "Warning: Failed to create backup at {}: {}",
                backup_path.display(),
                e
            );
        } else {
            println!("Created backup: {}", backup_path.display());
        }
    }

    // Write output file
    if let Err(e) = fs::write(output_path, &output_data) {
        eprintln!("Error writing to {}: {}", output_path.display(), e);
        process::exit(1);
    }

    println!("Successfully wrote: {}", output_path.display());
    process::exit(0);
}

fn handle_single_arch(mut builder: Builder, opts: Options) -> ! {

    // Apply modifications
    let mut modified = false;

    // Add rpaths
    for rpath in &opts.add_rpaths {
        println!("Adding rpath: {}", rpath);
        builder.add_rpath(rpath);
        modified = true;
    }

    // Delete rpaths
    for rpath in &opts.delete_rpaths {
        println!("Deleting rpath: {}", rpath);
        builder.remove_rpath(rpath);
        modified = true;
    }

    // Change rpaths
    for (old, new) in &opts.change_rpaths {
        println!("Changing rpath from '{}' to '{}'", old, new);
        builder.remove_rpath(old);
        builder.add_rpath(new);
        modified = true;
    }

    // Change dylib dependencies
    for (old, new) in &opts.change_dylibs {
        println!("Changing dependency from '{}' to '{}'", old, new);
        builder.change_dependency(old, new);
        modified = true;
    }

    // Change dylib ID
    if let Some(new_id) = &opts.new_id {
        println!("Changing dylib ID to: {}", new_id);
        builder.set_install_name(new_id, 0x10000, 0x10000);
        modified = true;
    }

    // If list mode, show info and optionally exit
    if opts.list_only {
        if modified {
            println!("Modified state:");
        }
        list_info(&builder);

        if !modified {
            process::exit(0); // Read-only list, exit early
        }
    }

    if !modified {
        eprintln!("Warning: No modifications specified");
        process::exit(0);
    }

    // Write output
    let output_data = match builder.write() {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error writing Mach-O file: {}", e);
            process::exit(1);
        }
    };

    let output_path = opts.output_file.as_ref().unwrap_or(&opts.input_file);

    // Create backup if requested
    if opts.backup && output_path == &opts.input_file {
        let backup_path = {
            let mut p = opts.input_file.clone();
            let mut filename = p.file_name().unwrap().to_os_string();
            filename.push(".bak");
            p.set_file_name(filename);
            p
        };
        if let Err(e) = fs::copy(&opts.input_file, &backup_path) {
            eprintln!(
                "Warning: Failed to create backup at {}: {}",
                backup_path.display(),
                e
            );
        } else {
            println!("Created backup: {}", backup_path.display());
        }
    }

    // Write output file
    if let Err(e) = fs::write(output_path, &output_data) {
        eprintln!("Error writing to {}: {}", output_path.display(), e);
        process::exit(1);
    }

    println!("Successfully wrote: {}", output_path.display());
    process::exit(0);
}
