//! This module provides a [`Builder`] for reading, modifying, and then writing Mach-O files.
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
            marker: PhantomData,
        };

        // Parse load commands
        let mut commands = header.load_commands(endian, data, 0)?;
        while let Some(command) = commands.next()? {
            match command.variant()? {
                LoadCommandVariant::Segment32(segment, section_data) => {
                    builder.read_segment_32(endian, data, segment, section_data)?;
                }
                LoadCommandVariant::Segment64(segment, section_data) => {
                    builder.read_segment_64(endian, data, segment, section_data)?;
                }
                LoadCommandVariant::Dylib(dylib_cmd) => {
                    let name = command.string(endian, dylib_cmd.dylib.name)?;
                    builder.load_commands.load_dylibs.push(LoadDylib {
                        dylib: MachODylib {
                            name: name.to_vec(),
                            timestamp: dylib_cmd.dylib.timestamp.get(endian),
                            current_version: dylib_cmd.dylib.current_version.get(endian),
                            compatibility_version: dylib_cmd.dylib.compatibility_version.get(endian),
                        },
                    });
                }
                LoadCommandVariant::IdDylib(dylib_cmd) => {
                    let name = command.string(endian, dylib_cmd.dylib.name)?;
                    builder.load_commands.id_dylib = Some(IdDylib {
                        dylib: MachODylib {
                            name: name.to_vec(),
                            timestamp: dylib_cmd.dylib.timestamp.get(endian),
                            current_version: dylib_cmd.dylib.current_version.get(endian),
                            compatibility_version: dylib_cmd.dylib.compatibility_version.get(endian),
                        },
                    });
                }
                LoadCommandVariant::LoadDylinker(dylinker_cmd) => {
                    let path = command.string(endian, dylinker_cmd.name)?;
                    builder.load_commands.load_dylinker = Some(LoadDylinker {
                        dylinker: MachOLoadDylinker {
                            dylinker: path.to_vec(),
                        },
                    });
                }
                LoadCommandVariant::EntryPoint(entry_cmd) => {
                    builder.load_commands.entry_point = Some(EntryPoint {
                        entry: MachOEntryPoint {
                            entryoff: entry_cmd.entryoff.get(endian),
                            stacksize: entry_cmd.stacksize.get(endian),
                        },
                    });
                }
                LoadCommandVariant::Rpath(rpath_cmd) => {
                    let path = command.string(endian, rpath_cmd.path)?;
                    builder.load_commands.rpaths.push(Rpath {
                        rpath: MachORpath {
                            path: path.to_vec(),
                        },
                    });
                }
                LoadCommandVariant::Uuid(uuid_cmd) => {
                    builder.load_commands.uuid = Some(Uuid {
                        uuid: uuid_cmd.uuid,
                    });
                }
                LoadCommandVariant::BuildVersion(build_cmd) => {
                    // TODO: Parse build version tools
                    builder.load_commands.build_version = Some(BuildVersion {
                        platform: build_cmd.platform.get(endian),
                        minos: build_cmd.minos.get(endian),
                        sdk: build_cmd.sdk.get(endian),
                        tools: Vec::new(),
                    });
                }
                LoadCommandVariant::SourceVersion(source_cmd) => {
                    builder.load_commands.source_version = Some(SourceVersion {
                        version: source_cmd.version.get(endian),
                    });
                }
                LoadCommandVariant::Symtab(_) => {
                    // TODO: Handle symbol table
                }
                LoadCommandVariant::Dysymtab(_) => {
                    // TODO: Handle dynamic symbol table
                }
                _ => {
                    // Ignore other load commands for now
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

        self.segments.push(Segment {
            id,
            delete: false,
            name: name.to_vec(),
            vmaddr: segment.vmaddr.get(endian) as u64,
            vmsize: segment.vmsize.get(endian) as u64,
            fileoff: segment.fileoff.get(endian) as u64,
            filesize: segment.filesize.get(endian) as u64,
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

        self.segments.push(Segment {
            id,
            delete: false,
            name: name.to_vec(),
            vmaddr: segment.vmaddr.get(endian),
            vmsize: segment.vmsize.get(endian),
            fileoff: segment.fileoff.get(endian),
            filesize: segment.filesize.get(endian),
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
    pub fn write(self) -> Result<Vec<u8>> {
        // Use the existing write::Object to do the actual writing
        let mut object = write::Object::new(
            crate::BinaryFormat::MachO,
            self.architecture,
            self.endian,
        );

        // Set file type
        object.set_macho_file_type(self.header.file_type);

        // Set load commands
        if let Some(dylinker) = self.load_commands.load_dylinker {
            object.set_macho_load_dylinker(dylinker.dylinker);
        }

        if let Some(entry_point) = self.load_commands.entry_point {
            object.set_macho_entry_point(entry_point.entry);
        }

        for rpath in self.load_commands.rpaths {
            object.add_macho_rpath(rpath.rpath);
        }

        if let Some(id_dylib) = self.load_commands.id_dylib {
            object.set_macho_id_dylib(id_dylib.dylib);
        }

        for load_dylib in self.load_commands.load_dylibs {
            object.add_macho_load_dylib(load_dylib.dylib);
        }

        // TODO: Add segments and sections from self.segments

        // Write the object
        let bytes = object.write()?;
        Ok(bytes)
    }

    /// Add an RPATH to the file.
    pub fn add_rpath(&mut self, path: &str) {
        self.load_commands.rpaths.push(Rpath {
            rpath: MachORpath::from_str(path),
        });
    }

    /// Remove all RPATHs that match the given path.
    pub fn remove_rpath(&mut self, path: &str) {
        self.load_commands
            .rpaths
            .retain(|r| r.rpath.path != path.as_bytes());
    }

    /// Get all RPATHs.
    pub fn rpaths(&self) -> impl Iterator<Item = &[u8]> {
        self.load_commands.rpaths.iter().map(|r| r.rpath.path.as_slice())
    }

    /// Set the install name for a dylib.
    pub fn set_install_name(&mut self, name: &str, current_version: u32, compatibility_version: u32) {
        self.load_commands.id_dylib = Some(IdDylib {
            dylib: MachODylib {
                name: name.as_bytes().to_vec(),
                timestamp: 2, // Standard timestamp value
                current_version,
                compatibility_version,
            },
        });
    }

    /// Get the install name if this is a dylib.
    pub fn install_name(&self) -> Option<&[u8]> {
        self.load_commands.id_dylib.as_ref().map(|d| d.dylib.name.as_slice())
    }

    /// Add a library dependency.
    pub fn add_dependency(&mut self, name: &str) {
        self.load_commands.load_dylibs.push(LoadDylib {
            dylib: MachODylib::from_str(name),
        });
    }

    /// Remove all library dependencies that match the given name.
    pub fn remove_dependency(&mut self, name: &str) {
        self.load_commands
            .load_dylibs
            .retain(|d| d.dylib.name != name.as_bytes());
    }

    /// Get all library dependencies.
    pub fn dependencies(&self) -> impl Iterator<Item = &[u8]> {
        self.load_commands
            .load_dylibs
            .iter()
            .map(|d| d.dylib.name.as_slice())
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

/// Load commands in a Mach-O file.
#[derive(Debug, Default)]
pub struct LoadCommands<'data> {
    /// LC_LOAD_DYLINKER command.
    pub load_dylinker: Option<LoadDylinker>,
    /// LC_MAIN command.
    pub entry_point: Option<EntryPoint>,
    /// LC_RPATH commands.
    pub rpaths: Vec<Rpath>,
    /// LC_ID_DYLIB command.
    pub id_dylib: Option<IdDylib>,
    /// LC_LOAD_DYLIB commands.
    pub load_dylibs: Vec<LoadDylib>,
    /// LC_UUID command.
    pub uuid: Option<Uuid>,
    /// LC_BUILD_VERSION command.
    pub build_version: Option<BuildVersion>,
    /// LC_SOURCE_VERSION command.
    pub source_version: Option<SourceVersion>,
    marker: PhantomData<&'data ()>,
}

impl<'data> LoadCommands<'data> {
    fn new() -> Self {
        Self {
            load_dylinker: None,
            entry_point: None,
            rpaths: Vec::new(),
            id_dylib: None,
            load_dylibs: Vec::new(),
            uuid: None,
            build_version: None,
            source_version: None,
            marker: PhantomData,
        }
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
