//! This module provides a [`Builder`] for reading, modifying, and then writing Mach-O files.

mod lc_writer;
pub use lc_writer::*;

use alloc::vec::Vec;
use core::marker::PhantomData;

use crate::build::{Bytes, Error, Id, IdPrivate, Item, Result, Table};
use crate::macho;
use crate::read::macho::{
    LoadCommandVariant, MachHeader, Section as SectionTrait, Segment as SegmentTrait,
};
use crate::read::{FileKind, ReadRef};
use crate::write;
use crate::write::{MachODylib, MachOEntryPoint, MachOLoadDylinker, MachORpath};
use crate::{Architecture, Endianness};

/// A builder for reading, modifying, and then writing Mach-O files.
///
/// Public fields are available for modifying the values that will be written.
/// Methods are available to manipulate load commands and sections.
#[derive(Debug)]
pub struct Builder<'data> {
    /// The endianness.
    pub endian: Endianness,
    /// The architecture.
    pub architecture: Architecture,
    /// Whether file is 64-bit.
    pub is_64: bool,
    /// The file header.
    pub header: Header,
    /// The load commands.
    pub load_commands: LoadCommands<'data>,
    /// The segments and sections.
    pub segments: Segments<'data>,
    /// The original binary data (for preserving segments and code).
    original_data: Option<&'data [u8]>,
    /// The minimum file offset where actual segment data begins.
    /// This is used to calculate available slack space for growing load commands.
    pub first_segment_data_offset: u64,
    marker: PhantomData<()>,
}

impl<'data> Builder<'data> {
    /// Create a new Mach-O builder.
    pub fn new(endian: Endianness, architecture: Architecture, is_64: bool) -> Self {
        Self {
            endian,
            architecture,
            is_64,
            header: Header::default(),
            load_commands: LoadCommands::new(),
            segments: Segments::new(),
            original_data: None,
            first_segment_data_offset: 0,
            marker: PhantomData,
        }
    }

    /// Read the Mach-O file from file data.
    pub fn read<R: ReadRef<'data>>(data: R) -> Result<Self> {
        match FileKind::parse(data)? {
            FileKind::MachO32 => Self::read32(data),
            FileKind::MachO64 => Self::read64(data),
            FileKind::MachOFat32 | FileKind::MachOFat64 => {
                Err(Error::new("Fat Mach-O files not supported yet"))
            }
            #[allow(unreachable_patterns)]
            _ => Err(Error::new("Not a Mach-O file")),
        }
    }

    /// Read a 32-bit Mach-O file from file data.
    pub fn read32<R: ReadRef<'data>>(data: R) -> Result<Self> {
        Self::read_file::<macho::MachHeader32<Endianness>, R>(data)
    }

    /// Read a 64-bit Mach-O file from file data.
    pub fn read64<R: ReadRef<'data>>(data: R) -> Result<Self> {
        Self::read_file::<macho::MachHeader64<Endianness>, R>(data)
    }

    fn read_file<Mach, R>(data: R) -> Result<Self>
    where
        Mach: MachHeader<Endian = Endianness>,
        R: ReadRef<'data>,
    {
        let header = Mach::parse(data, 0)?;
        let endian = header.endian()?;
        let is_64 = header.is_type_64();

        // Parse the architecture from the cputype
        let cputype = header.cputype(endian);
        let architecture = match cputype {
            macho::CPU_TYPE_ARM => Architecture::Arm,
            macho::CPU_TYPE_ARM64 => Architecture::Aarch64,
            macho::CPU_TYPE_ARM64_32 => Architecture::Aarch64_Ilp32,
            macho::CPU_TYPE_X86 => Architecture::I386,
            macho::CPU_TYPE_X86_64 => Architecture::X86_64,
            macho::CPU_TYPE_MIPS => Architecture::Mips,
            _ => Architecture::Unknown,
        };

        // Store the original data as bytes
        let file_len = data.len().map_err(|_| Error::new("Failed to get file length"))?;
        let data_bytes = data.read_bytes_at(0, file_len)
            .map_err(|_| Error::new("Failed to read binary data"))?;

        let mut builder = Builder {
            endian,
            architecture,
            is_64,
            header: Header {
                file_type: header.filetype(endian),
                cpu_type: header.cputype(endian),
                cpu_subtype: header.cpusubtype(endian),
                flags: header.flags(endian),
            },
            load_commands: LoadCommands::new(),
            segments: Segments::new(),
            original_data: Some(data_bytes),
            first_segment_data_offset: 0,
            marker: PhantomData,
        };

        // Parse load commands
        let mut commands = header.load_commands(endian, data, 0)?;
        while let Some(command) = commands.next()? {
            match command.variant()? {
                LoadCommandVariant::Segment32(segment, section_data) => {
                    builder.read_segment_32(endian, data, segment, section_data)?;
                    // Store as raw to preserve in original order
                    builder.load_commands.commands.push(LoadCommand::Raw {
                        cmd: command.cmd(),
                        data: command.raw_data(),
                    });
                }
                LoadCommandVariant::Segment64(segment, section_data) => {
                    builder.read_segment_64(endian, data, segment, section_data)?;
                    // Store as raw to preserve in original order
                    builder.load_commands.commands.push(LoadCommand::Raw {
                        cmd: command.cmd(),
                        data: command.raw_data(),
                    });
                }
                LoadCommandVariant::Dylib(dylib_cmd) => {
                    let name = command.string(endian, dylib_cmd.dylib.name)?;
                    builder.load_commands.commands.push(LoadCommand::LoadDylib(LoadDylib {
                        dylib: MachODylib {
                            name: name.to_vec(),
                            timestamp: dylib_cmd.dylib.timestamp.get(endian),
                            current_version: dylib_cmd.dylib.current_version.get(endian),
                            compatibility_version: dylib_cmd.dylib.compatibility_version.get(endian),
                        },
                    }));
                }
                LoadCommandVariant::IdDylib(dylib_cmd) => {
                    let name = command.string(endian, dylib_cmd.dylib.name)?;
                    builder.load_commands.commands.push(LoadCommand::IdDylib(IdDylib {
                        dylib: MachODylib {
                            name: name.to_vec(),
                            timestamp: dylib_cmd.dylib.timestamp.get(endian),
                            current_version: dylib_cmd.dylib.current_version.get(endian),
                            compatibility_version: dylib_cmd.dylib.compatibility_version.get(endian),
                        },
                    }));
                }
                LoadCommandVariant::Rpath(rpath_cmd) => {
                    let path = command.string(endian, rpath_cmd.path)?;
                    builder.load_commands.commands.push(LoadCommand::Rpath(Rpath {
                        rpath: MachORpath {
                            path: path.to_vec(),
                        },
                    }));
                }
                // All other commands: store as raw to preserve exact order and structure
                LoadCommandVariant::Symtab(_)
                | LoadCommandVariant::Dysymtab(_)
                | LoadCommandVariant::DyldInfo(_)
                | LoadCommandVariant::LinkeditData(_)
                | LoadCommandVariant::TwolevelHints(_)
                | LoadCommandVariant::PrebindCksum(_)
                | LoadCommandVariant::Thread(_, _)
                | LoadCommandVariant::LoadDylinker(_)
                | LoadCommandVariant::IdDylinker(_)
                | LoadCommandVariant::PreboundDylib(_)
                | LoadCommandVariant::Routines32(_)
                | LoadCommandVariant::Routines64(_)
                | LoadCommandVariant::SubFramework(_)
                | LoadCommandVariant::SubUmbrella(_)
                | LoadCommandVariant::SubClient(_)
                | LoadCommandVariant::SubLibrary(_)
                | LoadCommandVariant::EncryptionInfo32(_)
                | LoadCommandVariant::EncryptionInfo64(_)
                | LoadCommandVariant::DyldEnvironment(_)
                | LoadCommandVariant::LinkerOption(_)
                | LoadCommandVariant::Note(_)
                | LoadCommandVariant::FilesetEntry(_)
                | LoadCommandVariant::Uuid(_)
                | LoadCommandVariant::BuildVersion(_)
                | LoadCommandVariant::SourceVersion(_)
                | LoadCommandVariant::VersionMin(_)
                | LoadCommandVariant::EntryPoint(_)
                | LoadCommandVariant::Other => {
                    // Store all other commands as raw to preserve exact structure
                    builder.load_commands.commands.push(LoadCommand::Raw {
                        cmd: command.cmd(),
                        data: command.raw_data(),
                    });
                }
            }
        }

        Ok(builder)
    }

    fn read_segment_32<R: ReadRef<'data>>(
        &mut self,
        endian: Endianness,
        _data: R,
        segment: &macho::SegmentCommand32<Endianness>,
        section_data: &'data [u8],
    ) -> Result<()> {
        let id = self.segments.next_id();
        let name = segment.name();
        let sections = segment.sections(endian, section_data)?;

        let fileoff = segment.fileoff.get(endian) as u64;
        let filesize = segment.filesize.get(endian) as u64;

        // Track the first offset where actual section data begins
        // We need to look at sections, not just segments, because the __TEXT segment
        // includes the header and load commands at its start
        for section in sections {
            let section_offset = section.offset.get(endian) as u64;
            let section_size = section.size.get(endian) as u64;

            if section_size > 0 && section_offset > 0 {
                if self.first_segment_data_offset == 0 || section_offset < self.first_segment_data_offset {
                    self.first_segment_data_offset = section_offset;
                }
            }
        }

        self.segments.push(Segment {
            id,
            delete: false,
            name: name.to_vec(),
            vmaddr: segment.vmaddr.get(endian) as u64,
            vmsize: segment.vmsize.get(endian) as u64,
            fileoff,
            filesize,
            maxprot: segment.maxprot.get(endian),
            initprot: segment.initprot.get(endian),
            flags: segment.flags.get(endian),
            sections: sections
                .iter()
                .map(|section| Section {
                    name: section.name().to_vec(),
                    segment_name: section.segment_name().to_vec(),
                    addr: section.addr.get(endian) as u64,
                    size: section.size.get(endian) as u64,
                    offset: section.offset.get(endian),
                    align: section.align.get(endian),
                    reloff: section.reloff.get(endian),
                    nreloc: section.nreloc.get(endian),
                    flags: section.flags.get(endian),
                    data: Bytes::from(&[][..]), // TODO: Store actual data
                })
                .collect(),
        });

        Ok(())
    }

    fn read_segment_64<R: ReadRef<'data>>(
        &mut self,
        endian: Endianness,
        _data: R,
        segment: &macho::SegmentCommand64<Endianness>,
        section_data: &'data [u8],
    ) -> Result<()> {
        let id = self.segments.next_id();
        let name = segment.name();
        let sections = segment.sections(endian, section_data)?;

        let fileoff = segment.fileoff.get(endian);
        let filesize = segment.filesize.get(endian);

        // Track the first offset where actual section data begins
        // We need to look at sections, not just segments, because the __TEXT segment
        // includes the header and load commands at its start
        for section in sections {
            let section_offset = section.offset.get(endian) as u64;
            let section_size = section.size.get(endian);

            if section_size > 0 && section_offset > 0 {
                if self.first_segment_data_offset == 0 || section_offset < self.first_segment_data_offset {
                    self.first_segment_data_offset = section_offset;
                }
            }
        }

        self.segments.push(Segment {
            id,
            delete: false,
            name: name.to_vec(),
            vmaddr: segment.vmaddr.get(endian),
            vmsize: segment.vmsize.get(endian),
            fileoff,
            filesize,
            maxprot: segment.maxprot.get(endian),
            initprot: segment.initprot.get(endian),
            flags: segment.flags.get(endian),
            sections: sections
                .iter()
                .map(|section| Section {
                    name: section.name().to_vec(),
                    segment_name: section.segment_name().to_vec(),
                    addr: section.addr.get(endian),
                    size: section.size.get(endian),
                    offset: section.offset.get(endian),
                    align: section.align.get(endian),
                    reloff: section.reloff.get(endian),
                    nreloc: section.nreloc.get(endian),
                    flags: section.flags.get(endian),
                    data: Bytes::from(&[][..]), // TODO: Store actual data
                })
                .collect(),
        });

        Ok(())
    }

    /// Write the Mach-O file to a buffer.
    ///
    /// Modifies the original binary data in-place, preserving all segments and code.
    /// This only works for binaries created from `Builder::read()`.
    pub fn write(self) -> Result<Vec<u8>> {
        // If we have original data, modify it in-place
        if let Some(original) = self.original_data {
            return self.write_in_place(original);
        }

        // For now, we only support in-place modification (Phase 3)
        // Creating new binaries from scratch will be added in a future phase
        Err(Error::new("Creating new binaries from scratch is not yet supported. Use Builder::read() first."))
    }

    /// Write by modifying the original binary in-place.
    ///
    /// This approach preserves all segment data and only modifies load commands.
    /// It works similar to how install_name_tool works.
    ///
    /// Handles three cases:
    /// - Case 1: New commands are same size → Direct replacement
    /// - Case 2: New commands are smaller → Replace and pad with zeros
    /// - Case 3: New commands are larger → Grow into slack space before segments
    ///
    /// If new commands exceed available slack space, returns an error.
    fn write_in_place(self, original: &[u8]) -> Result<Vec<u8>> {
        // Calculate the header size
        let header_size = if self.is_64 {
            core::mem::size_of::<macho::MachHeader64<Endianness>>()
        } else {
            core::mem::size_of::<macho::MachHeader32<Endianness>>()
        };

        // Read original header to get old load command section size
        if original.len() < header_size {
            return Err(Error::new("Binary too small for header"));
        }

        // Extract old sizeofcmds from header
        // Field is at offset 20 for both 32-bit and 64-bit headers
        let old_sizeofcmds = match self.endian {
            Endianness::Little => u32::from_le_bytes([
                original[20], original[21], original[22], original[23]
            ]),
            Endianness::Big => u32::from_be_bytes([
                original[20], original[21], original[22], original[23]
            ]),
        };

        // Build new load commands using LoadCommandWriter
        let (new_load_commands, new_ncmds) = self.build_load_commands();
        let new_sizeofcmds = new_load_commands.len() as u32;

        // Check if new load commands fit
        // Case 1: Fit in original space (simple)
        if new_sizeofcmds <= old_sizeofcmds {
            // Simple case - commands fit in original space
        } else {
            // Case 2: Need to use slack space before first segment
            let new_lc_end = header_size as u64 + new_sizeofcmds as u64;

            if new_lc_end > self.first_segment_data_offset {
                return Err(Error::new(format!(
                    "New load commands ({} bytes) exceed available space before first segment (max {} bytes). \
                     Full segment relocation not yet implemented.",
                    new_sizeofcmds,
                    self.first_segment_data_offset.saturating_sub(header_size as u64)
                )));
            }

            // Good! We can grow into the slack space
        }

        // Start with a copy of the original data
        let mut output = original.to_vec();

        // Replace the load command section
        let lc_start = header_size;
        let lc_end = lc_start + new_sizeofcmds as usize;
        let old_lc_end = lc_start + old_sizeofcmds as usize;

        // Safety check for the maximum we might write
        if lc_end > output.len() {
            return Err(Error::new("New load commands extend beyond file"));
        }

        // Copy new load commands
        output[lc_start..lc_end].copy_from_slice(&new_load_commands);

        // Handle different size cases
        if new_sizeofcmds < old_sizeofcmds {
            // Case: Commands shrank - zero out the gap
            for i in lc_end..old_lc_end {
                output[i] = 0;
            }
        } else if new_sizeofcmds > old_sizeofcmds {
            // Case: Commands grew into slack space
            // The new commands have already overwritten the slack space
            // No additional work needed - slack space is now part of load commands
        }
        // else: exact same size, no action needed

        // Update the header with new command count and size
        // ncmds at offset 16
        let ncmds_bytes = match self.endian {
            Endianness::Little => new_ncmds.to_le_bytes(),
            Endianness::Big => new_ncmds.to_be_bytes(),
        };
        output[16..20].copy_from_slice(&ncmds_bytes);

        // sizeofcmds at offset 20
        let sizeofcmds_bytes = match self.endian {
            Endianness::Little => new_sizeofcmds.to_le_bytes(),
            Endianness::Big => new_sizeofcmds.to_be_bytes(),
        };
        output[20..24].copy_from_slice(&sizeofcmds_bytes);

        Ok(output)
    }

    /// Build all load commands into a byte buffer.
    ///
    /// Returns (buffer, command_count).
    ///
    /// IMPORTANT: Commands must be written in the same order they were read to ensure
    /// bit-for-bit compatibility. Unknown commands (segments, symtab, etc.) are written first
    /// as they typically appear first in the original binary.
    fn build_load_commands(&self) -> (Vec<u8>, u32) {
        let mut writer = LoadCommandWriter::new(self.endian);

        // Write all commands in their original order
        // This preserves the exact structure for bit-for-bit copies
        for command in &self.load_commands.commands {
            match command {
                LoadCommand::Rpath(rpath) => {
                    writer.write_rpath(&rpath.rpath.path);
                }
                LoadCommand::LoadDylib(load_dylib) => {
                    writer.write_load_dylib(
                        &load_dylib.dylib.name,
                        load_dylib.dylib.timestamp,
                        load_dylib.dylib.current_version,
                        load_dylib.dylib.compatibility_version,
                    );
                }
                LoadCommand::IdDylib(id_dylib) => {
                    writer.write_id_dylib(
                        &id_dylib.dylib.name,
                        id_dylib.dylib.timestamp,
                        id_dylib.dylib.current_version,
                        id_dylib.dylib.compatibility_version,
                    );
                }
                LoadCommand::Raw { cmd: _, data } => {
                    writer.write_raw_command(data);
                }
            }
        }

        writer.into_bytes()
    }

    /// Add an RPATH to the file.
    pub fn add_rpath(&mut self, path: &str) {
        self.load_commands.add_rpath(path.as_bytes().to_vec());
    }

    /// Remove all RPATHs that match the given path.
    pub fn remove_rpath(&mut self, path: &str) {
        self.load_commands.remove_rpath(path.as_bytes());
    }

    /// Get all RPATHs.
    pub fn rpaths(&self) -> impl Iterator<Item = &[u8]> {
        self.load_commands.commands.iter().filter_map(|cmd| {
            if let LoadCommand::Rpath(rpath) = cmd {
                Some(rpath.rpath.path.as_slice())
            } else {
                None
            }
        })
    }

    /// Set the install name for a dylib.
    pub fn set_install_name(&mut self, name: &str, current_version: u32, compatibility_version: u32) {
        self.load_commands.set_id_dylib(MachODylib {
            name: name.as_bytes().to_vec(),
            timestamp: 2, // Standard timestamp value
            current_version,
            compatibility_version,
        });
    }

    /// Get the install name if this is a dylib.
    pub fn install_name(&self) -> Option<&[u8]> {
        self.load_commands.commands.iter().find_map(|cmd| {
            if let LoadCommand::IdDylib(id_dylib) = cmd {
                Some(id_dylib.dylib.name.as_slice())
            } else {
                None
            }
        })
    }

    /// Add a library dependency.
    pub fn add_dependency(&mut self, name: &str) {
        self.load_commands.add_load_dylib(MachODylib::from_str(name));
    }

    /// Remove all library dependencies that match the given name.
    pub fn remove_dependency(&mut self, name: &str) {
        self.load_commands.remove_load_dylib(name.as_bytes());
    }

    /// Change a library dependency from old to new (preserves order).
    ///
    /// This replaces the dependency in-place, maintaining the original position
    /// in the dependency list (similar to install_name_tool -change).
    pub fn change_dependency(&mut self, old_name: &str, new_name: &str) {
        for cmd in &mut self.load_commands.commands {
            if let LoadCommand::LoadDylib(dylib) = cmd {
                if dylib.dylib.name == old_name.as_bytes() {
                    dylib.dylib.name = new_name.as_bytes().to_vec();
                }
            }
        }
    }

    /// Get all library dependencies.
    pub fn dependencies(&self) -> impl Iterator<Item = &[u8]> {
        self.load_commands.commands.iter().filter_map(|cmd| {
            if let LoadCommand::LoadDylib(dylib) = cmd {
                Some(dylib.dylib.name.as_slice())
            } else {
                None
            }
        })
    }
}

/// The Mach-O file header.
#[derive(Debug, Clone, Default)]
pub struct Header {
    /// The file type (MH_OBJECT, MH_EXECUTE, MH_DYLIB, etc.)
    pub file_type: u32,
    /// The CPU type.
    pub cpu_type: u32,
    /// The CPU subtype.
    pub cpu_subtype: u32,
    /// The header flags.
    pub flags: u32,
}

/// An individual load command, preserving order.
#[derive(Debug, Clone)]
pub enum LoadCommand<'data> {
    /// LC_RPATH command (modifiable).
    Rpath(Rpath),
    /// LC_LOAD_DYLIB command (modifiable).
    LoadDylib(LoadDylib),
    /// LC_ID_DYLIB command (modifiable).
    IdDylib(IdDylib),
    /// All other commands preserved as raw bytes (segments, symtab, uuid, build_version, etc.).
    /// This preserves exact order and structure for bit-for-bit copies.
    Raw {
        /// The load command type.
        cmd: u32,
        /// The raw command data including the load command header.
        data: &'data [u8]
    },
}

/// Load commands in a Mach-O file.
#[derive(Debug)]

pub struct LoadCommands<'data> {
    /// All load commands in their original order.
    /// This preserves the exact order of commands for bit-for-bit copies.
    pub commands: Vec<LoadCommand<'data>>,
    marker: PhantomData<&'data ()>,
}

impl<'data> LoadCommands<'data> {
    fn new() -> Self {
        Self {
            commands: Vec::new(),
            marker: PhantomData,
        }
    }

    /// Add an RPATH command.
    pub fn add_rpath(&mut self, path: Vec<u8>) {
        self.commands.push(LoadCommand::Rpath(Rpath {
            rpath: MachORpath { path },
        }));
    }

    /// Add a LOAD_DYLIB command.
    pub fn add_load_dylib(&mut self, dylib: MachODylib) {
        self.commands.push(LoadCommand::LoadDylib(LoadDylib { dylib }));
    }

    /// Set the ID_DYLIB command (replaces existing if present).
    pub fn set_id_dylib(&mut self, dylib: MachODylib) {
        // Remove any existing ID_DYLIB
        self.commands.retain(|cmd| !matches!(cmd, LoadCommand::IdDylib(_)));
        // Add new one at the end (or we could insert at a specific position)
        self.commands.push(LoadCommand::IdDylib(IdDylib { dylib }));
    }

    /// Remove all RPATH commands matching the given path.
    pub fn remove_rpath(&mut self, path: &[u8]) {
        self.commands.retain(|cmd| {
            !matches!(cmd, LoadCommand::Rpath(rpath) if rpath.rpath.path == path)
        });
    }

    /// Remove all LOAD_DYLIB commands matching the given name.
    pub fn remove_load_dylib(&mut self, name: &[u8]) {
        self.commands.retain(|cmd| {
            !matches!(cmd, LoadCommand::LoadDylib(dylib) if dylib.dylib.name == name)
        });
    }
}

/// LC_LOAD_DYLINKER load command.
#[derive(Debug, Clone)]
pub struct LoadDylinker {
    /// The dynamic linker.
    pub dylinker: MachOLoadDylinker,
}

/// LC_MAIN load command.
#[derive(Debug, Clone, Copy)]
pub struct EntryPoint {
    /// The entry point.
    pub entry: MachOEntryPoint,
}

/// LC_RPATH load command.
#[derive(Debug, Clone)]
pub struct Rpath {
    /// The rpath.
    pub rpath: MachORpath,
}

/// LC_ID_DYLIB load command.
#[derive(Debug, Clone)]
pub struct IdDylib {
    /// The dylib information.
    pub dylib: MachODylib,
}

/// LC_LOAD_DYLIB load command.
#[derive(Debug, Clone)]
pub struct LoadDylib {
    /// The dylib information.
    pub dylib: MachODylib,
}

/// LC_UUID load command.
#[derive(Debug, Clone, Copy)]
pub struct Uuid {
    /// The UUID bytes.
    pub uuid: [u8; 16],
}

/// LC_BUILD_VERSION load command.
#[derive(Debug, Clone)]
pub struct BuildVersion {
    /// The platform.
    pub platform: u32,
    /// The minimum OS version.
    pub minos: u32,
    /// The SDK version.
    pub sdk: u32,
    /// The build tools.
    pub tools: Vec<BuildVersionTool>,
}

/// Build version tool information.
#[derive(Debug, Clone, Copy)]
pub struct BuildVersionTool {
    /// The tool identifier.
    pub tool: u32,
    /// The tool version.
    pub version: u32,
}

/// LC_SOURCE_VERSION load command.
#[derive(Debug, Clone, Copy)]
pub struct SourceVersion {
    /// The source version.
    pub version: u64,
}

/// LC_VERSION_MIN_* load command.
#[derive(Debug, Clone, Copy)]
pub struct VersionMin {
    /// The minimum OS version.
    pub version: u32,
    /// The SDK version.
    pub sdk: u32,
}

/// An unknown or unhandled load command.
///
/// This stores the raw bytes of load commands that we don't explicitly parse,
/// allowing them to be preserved during read-modify-write operations.
#[derive(Debug, Clone)]
pub struct UnknownCommand<'data> {
    /// The command type (e.g., LC_VERSION_MIN_MACOSX, LC_DYLD_INFO, etc.)
    pub cmd: u32,
    /// The raw command data including the header.
    pub data: &'data [u8],
}

/// Segments in a Mach-O file.
pub type Segments<'data> = Table<Segment<'data>>;

/// A segment ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SegmentId(usize);

impl Id for SegmentId {
    fn index(&self) -> usize {
        self.0
    }
}

impl IdPrivate for SegmentId {
    fn new(id: usize) -> Self {
        SegmentId(id)
    }
}

/// A segment in a Mach-O file.
#[derive(Debug)]
pub struct Segment<'data> {
    /// The segment ID.
    pub id: SegmentId,
    /// Whether this segment is deleted.
    pub delete: bool,
    /// The segment name.
    pub name: Vec<u8>,
    /// The virtual memory address.
    pub vmaddr: u64,
    /// The virtual memory size.
    pub vmsize: u64,
    /// The file offset.
    pub fileoff: u64,
    /// The file size.
    pub filesize: u64,
    /// The maximum protection.
    pub maxprot: u32,
    /// The initial protection.
    pub initprot: u32,
    /// The segment flags.
    pub flags: u32,
    /// The sections in this segment.
    pub sections: Vec<Section<'data>>,
}

impl<'data> Item for Segment<'data> {
    type Id = SegmentId;

    fn is_deleted(&self) -> bool {
        self.delete
    }
}

/// A section in a Mach-O segment.
#[derive(Debug)]
pub struct Section<'data> {
    /// The section name.
    pub name: Vec<u8>,
    /// The segment name.
    pub segment_name: Vec<u8>,
    /// The section address.
    pub addr: u64,
    /// The section size.
    pub size: u64,
    /// The file offset.
    pub offset: u32,
    /// The section alignment (power of 2).
    pub align: u32,
    /// The relocation offset.
    pub reloff: u32,
    /// The number of relocations.
    pub nreloc: u32,
    /// The section flags.
    pub flags: u32,
    /// The section data.
    pub data: Bytes<'data>,
}
