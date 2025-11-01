use core::mem;

use crate::endian::*;
use crate::macho;
use crate::write::string::*;
use crate::write::util::*;
use crate::write::*;

#[derive(Default, Clone, Copy)]
struct SectionOffsets {
    index: usize,
    offset: usize,
    address: u64,
    reloc_offset: usize,
    reloc_count: usize,
}

#[derive(Default, Clone, Copy)]
struct SymbolOffsets {
    index: usize,
    str_id: Option<StringId>,
}

/// The customizable portion of a [`macho::BuildVersionCommand`].
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive] // May want to add the tool list?
pub struct MachOBuildVersion {
    /// One of the `PLATFORM_` constants (for example,
    /// [`object::macho::PLATFORM_MACOS`](macho::PLATFORM_MACOS)).
    pub platform: u32,
    /// The minimum OS version, where `X.Y.Z` is encoded in nibbles as
    /// `xxxx.yy.zz`.
    pub minos: u32,
    /// The SDK version as `X.Y.Z`, where `X.Y.Z` is encoded in nibbles as
    /// `xxxx.yy.zz`.
    pub sdk: u32,
}

impl MachOBuildVersion {
    fn cmdsize(&self) -> u32 {
        // Same size for both endianness, and we don't have `ntools`.
        let sz = mem::size_of::<macho::BuildVersionCommand<Endianness>>();
        debug_assert!(sz <= u32::MAX as usize);
        sz as u32
    }
}

/// The customizable portion of a [`macho::DylinkerCommand`] for LC_LOAD_DYLINKER.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MachOLoadDylinker {
    /// Path to the dynamic linker (typically "/usr/lib/dyld" on macOS).
    pub dylinker: Vec<u8>,
}

impl MachOLoadDylinker {
    /// Create a new dylinker command with the standard macOS dylinker path.
    pub fn default_dyld() -> Self {
        Self {
            dylinker: b"/usr/lib/dyld".to_vec(),
        }
    }

    fn cmdsize(&self) -> u32 {
        // DylinkerCommand struct size + null-terminated string, rounded up to 8-byte alignment
        let base_size = mem::size_of::<macho::DylinkerCommand<Endianness>>();
        let string_size = self.dylinker.len() + 1; // +1 for null terminator
        let total = base_size + string_size;
        // Round up to 8-byte boundary
        let aligned = (total + 7) & !7;
        debug_assert!(aligned <= u32::MAX as usize);
        aligned as u32
    }
}

/// The customizable portion of a [`macho::EntryPointCommand`] for LC_MAIN.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MachOEntryPoint {
    /// File offset of the entry point (typically offset of main() function).
    pub entryoff: u64,
    /// Initial stack size (0 if not specified).
    pub stacksize: u64,
}

impl MachOEntryPoint {
    /// Create a new entry point command with the given offset and default stack size.
    pub fn new(entryoff: u64) -> Self {
        Self {
            entryoff,
            stacksize: 0,
        }
    }

    /// Create a new entry point command with the given offset and stack size.
    pub fn with_stacksize(entryoff: u64, stacksize: u64) -> Self {
        Self {
            entryoff,
            stacksize,
        }
    }

    fn cmdsize(&self) -> u32 {
        let sz = mem::size_of::<macho::EntryPointCommand<Endianness>>();
        debug_assert!(sz <= u32::MAX as usize);
        sz as u32
    }
}

/// The customizable portion of a [`macho::RpathCommand`] for LC_RPATH.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MachORpath {
    /// Runtime search path (e.g., "@executable_path/../Frameworks").
    pub path: Vec<u8>,
}

impl MachORpath {
    /// Create a new rpath command with the given path.
    pub fn new(path: Vec<u8>) -> Self {
        Self { path }
    }

    /// Create a new rpath command from a string slice.
    pub fn from_str(path: &str) -> Self {
        Self {
            path: path.as_bytes().to_vec(),
        }
    }

    fn cmdsize(&self) -> u32 {
        // RpathCommand struct size + null-terminated string, rounded up to 8-byte alignment
        let base_size = mem::size_of::<macho::RpathCommand<Endianness>>();
        let string_size = self.path.len() + 1; // +1 for null terminator
        let total = base_size + string_size;
        // Round up to 8-byte boundary
        let aligned = (total + 7) & !7;
        debug_assert!(aligned <= u32::MAX as usize);
        aligned as u32
    }
}

/// The customizable portion of a [`macho::DylibCommand`] for LC_ID_DYLIB or LC_LOAD_DYLIB.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MachODylib {
    /// Library path/name (e.g., "/usr/lib/libSystem.B.dylib" or "@rpath/MyFramework.framework/MyFramework").
    pub name: Vec<u8>,
    /// Library's build timestamp (typically 0 for modern dylibs).
    pub timestamp: u32,
    /// Current version number (encoded as X.Y.Z in nibbles).
    pub current_version: u32,
    /// Compatibility version number (encoded as X.Y.Z in nibbles).
    pub compatibility_version: u32,
}

impl MachODylib {
    /// Create a new dylib command with the given name and default version info.
    pub fn new(name: Vec<u8>) -> Self {
        Self {
            name,
            timestamp: 0,
            current_version: 0x10000, // 1.0.0
            compatibility_version: 0x10000, // 1.0.0
        }
    }

    /// Create a new dylib command from a string slice.
    pub fn from_str(name: &str) -> Self {
        Self::new(name.as_bytes().to_vec())
    }

    /// Create with specific version information.
    pub fn with_versions(name: Vec<u8>, current_version: u32, compatibility_version: u32) -> Self {
        Self {
            name,
            timestamp: 0,
            current_version,
            compatibility_version,
        }
    }

    /// Encode a version from major.minor.patch format.
    /// Version format: major is in the high 16 bits, minor in middle 8 bits, patch in low 8 bits.
    pub fn encode_version(major: u16, minor: u8, patch: u8) -> u32 {
        ((major as u32) << 16) | ((minor as u32) << 8) | (patch as u32)
    }

    fn cmdsize(&self) -> u32 {
        // DylibCommand struct size + null-terminated string, rounded up to 8-byte alignment
        let base_size = mem::size_of::<macho::DylibCommand<Endianness>>();
        let string_size = self.name.len() + 1; // +1 for null terminator
        let total = base_size + string_size;
        // Round up to 8-byte boundary
        let aligned = (total + 7) & !7;
        debug_assert!(aligned <= u32::MAX as usize);
        aligned as u32
    }
}

/// The customizable portion of a [`macho::UuidCommand`] for LC_UUID.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MachOUuid {
    /// 128-bit UUID (16 bytes).
    pub uuid: [u8; 16],
}

impl MachOUuid {
    /// Create a new UUID command with the given UUID bytes.
    pub fn new(uuid: [u8; 16]) -> Self {
        Self { uuid }
    }

    /// Generate a new random UUID (requires std for randomness).
    #[cfg(feature = "std")]
    pub fn random() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Simple UUID v4 generation using time-based randomness
        // This is not cryptographically secure but sufficient for build IDs
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let nanos = now.as_nanos();

        let mut uuid = [0u8; 16];
        uuid[0..8].copy_from_slice(&nanos.to_le_bytes()[0..8]);
        uuid[8..16].copy_from_slice(&nanos.to_be_bytes()[0..8]);

        // Set version to 4 (random)
        uuid[6] = (uuid[6] & 0x0F) | 0x40;
        // Set variant to RFC 4122
        uuid[8] = (uuid[8] & 0x3F) | 0x80;

        Self { uuid }
    }

    fn cmdsize(&self) -> u32 {
        let sz = mem::size_of::<macho::UuidCommand<Endianness>>();
        debug_assert!(sz <= u32::MAX as usize);
        sz as u32
    }
}

/// The customizable portion of a [`macho::SourceVersionCommand`] for LC_SOURCE_VERSION.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MachOSourceVersion {
    /// Source version encoded as A.B.C.D.E packed into 64 bits.
    /// A is 24 bits, B through E are 10 bits each.
    pub version: u64,
}

impl MachOSourceVersion {
    /// Create a new source version command with the given version.
    pub fn new(version: u64) -> Self {
        Self { version }
    }

    /// Encode a version from A.B.C.D.E format.
    /// A can be up to 16777215 (24 bits), B-E can each be up to 1023 (10 bits).
    pub fn encode_version(a: u32, b: u16, c: u16, d: u16, e: u16) -> u64 {
        ((a as u64 & 0xFFFFFF) << 40)
            | ((b as u64 & 0x3FF) << 30)
            | ((c as u64 & 0x3FF) << 20)
            | ((d as u64 & 0x3FF) << 10)
            | (e as u64 & 0x3FF)
    }

    fn cmdsize(&self) -> u32 {
        let sz = mem::size_of::<macho::SourceVersionCommand<Endianness>>();
        debug_assert!(sz <= u32::MAX as usize);
        sz as u32
    }
}

/// The customizable portion of a [`macho::VersionMinCommand`] for LC_VERSION_MIN_MACOSX, etc.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MachOVersionMin {
    /// Minimum OS version, where X.Y.Z is encoded in nibbles as xxxx.yy.zz.
    pub version: u32,
    /// SDK version as X.Y.Z, encoded in nibbles as xxxx.yy.zz.
    pub sdk: u32,
}

impl MachOVersionMin {
    /// Create a new version min command with the given version and SDK.
    pub fn new(version: u32, sdk: u32) -> Self {
        Self { version, sdk }
    }

    /// Encode a version from X.Y.Z format.
    pub fn encode_version(major: u16, minor: u8, patch: u8) -> u32 {
        ((major as u32) << 16) | ((minor as u32) << 8) | (patch as u32)
    }

    fn cmdsize(&self) -> u32 {
        let sz = mem::size_of::<macho::VersionMinCommand<Endianness>>();
        debug_assert!(sz <= u32::MAX as usize);
        sz as u32
    }
}

// Public methods.
impl<'a> Object<'a> {
    /// Specify the Mach-O CPU subtype.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_cpu_subtype(&mut self, cpu_subtype: u32) {
        self.macho_cpu_subtype = Some(cpu_subtype);
    }

    /// Specify information for a Mach-O `LC_BUILD_VERSION` command.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_build_version(&mut self, info: MachOBuildVersion) {
        self.macho_build_version = Some(info);
    }

    /// Specify the Mach-O file type.
    ///
    /// This can be used to create executables (`MH_EXECUTE`), dynamic libraries (`MH_DYLIB`),
    /// bundles (`MH_BUNDLE`), etc. If not set, defaults to `MH_OBJECT`.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_file_type(&mut self, file_type: u32) {
        self.macho_file_type = Some(file_type);
    }

    /// Specify the dynamic linker path for a Mach-O `LC_LOAD_DYLINKER` command.
    ///
    /// This is typically required for executables and specifies the path to the dynamic linker
    /// (usually "/usr/lib/dyld" on macOS).
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_load_dylinker(&mut self, dylinker: MachOLoadDylinker) {
        self.macho_load_dylinker = Some(dylinker);
    }

    /// Specify the entry point for a Mach-O `LC_MAIN` command.
    ///
    /// This is required for executables and specifies the file offset of the main() function.
    /// This is the modern replacement for LC_UNIXTHREAD.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_entry_point(&mut self, entry_point: MachOEntryPoint) {
        self.macho_entry_point = Some(entry_point);
    }

    /// Add a runtime search path for a Mach-O `LC_RPATH` command.
    ///
    /// Multiple rpaths can be added by calling this method multiple times.
    /// Common paths include "@executable_path/../Frameworks", "@loader_path", etc.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn add_macho_rpath(&mut self, rpath: MachORpath) {
        self.macho_rpaths.push(rpath);
    }

    /// Get the current list of Mach-O rpaths.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn macho_rpaths(&self) -> &[MachORpath] {
        &self.macho_rpaths
    }

    /// Get a mutable reference to the list of Mach-O rpaths.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn macho_rpaths_mut(&mut self) -> &mut Vec<MachORpath> {
        &mut self.macho_rpaths
    }

    /// Set the dylib identification for a Mach-O `LC_ID_DYLIB` command.
    ///
    /// This should only be set for MH_DYLIB file types to identify the library's install name.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_id_dylib(&mut self, dylib: MachODylib) {
        self.macho_id_dylib = Some(dylib);
    }

    /// Get the current dylib identification.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn macho_id_dylib(&self) -> Option<&MachODylib> {
        self.macho_id_dylib.as_ref()
    }

    /// Add a library dependency for a Mach-O `LC_LOAD_DYLIB` command.
    ///
    /// Multiple dependencies can be added by calling this method multiple times.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn add_macho_load_dylib(&mut self, dylib: MachODylib) {
        self.macho_load_dylibs.push(dylib);
    }

    /// Get the current list of library dependencies.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn macho_load_dylibs(&self) -> &[MachODylib] {
        &self.macho_load_dylibs
    }

    /// Get a mutable reference to the list of library dependencies.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn macho_load_dylibs_mut(&mut self) -> &mut Vec<MachODylib> {
        &mut self.macho_load_dylibs
    }

    /// Set a UUID for a Mach-O `LC_UUID` command.
    ///
    /// The UUID is a 128-bit unique identifier for the binary.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_uuid(&mut self, uuid: MachOUuid) {
        self.macho_uuid = Some(uuid);
    }

    /// Get the current UUID.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn macho_uuid(&self) -> Option<&MachOUuid> {
        self.macho_uuid.as_ref()
    }

    /// Set the source version for a Mach-O `LC_SOURCE_VERSION` command.
    ///
    /// The source version identifies the version of the source code used to build the binary.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_source_version(&mut self, version: MachOSourceVersion) {
        self.macho_source_version = Some(version);
    }

    /// Get the current source version.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn macho_source_version(&self) -> Option<&MachOSourceVersion> {
        self.macho_source_version.as_ref()
    }

    /// Set the minimum macOS version for a Mach-O `LC_VERSION_MIN_MACOSX` command.
    ///
    /// Specifies the minimum version of macOS required to run this binary.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_version_min_macosx(&mut self, version: MachOVersionMin) {
        self.macho_version_min_macosx = Some(version);
    }

    /// Set the minimum iOS version for a Mach-O `LC_VERSION_MIN_IPHONEOS` command.
    ///
    /// Specifies the minimum version of iOS required to run this binary.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_version_min_iphoneos(&mut self, version: MachOVersionMin) {
        self.macho_version_min_iphoneos = Some(version);
    }

    /// Set the minimum tvOS version for a Mach-O `LC_VERSION_MIN_TVOS` command.
    ///
    /// Specifies the minimum version of tvOS required to run this binary.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_version_min_tvos(&mut self, version: MachOVersionMin) {
        self.macho_version_min_tvos = Some(version);
    }

    /// Set the minimum watchOS version for a Mach-O `LC_VERSION_MIN_WATCHOS` command.
    ///
    /// Specifies the minimum version of watchOS required to run this binary.
    ///
    /// Requires `feature = "macho"`.
    #[inline]
    pub fn set_macho_version_min_watchos(&mut self, version: MachOVersionMin) {
        self.macho_version_min_watchos = Some(version);
    }
}

// Private methods.
impl<'a> Object<'a> {
    pub(crate) fn macho_segment_name(&self, segment: StandardSegment) -> &'static [u8] {
        match segment {
            StandardSegment::Text => &b"__TEXT"[..],
            StandardSegment::Data => &b"__DATA"[..],
            StandardSegment::Debug => &b"__DWARF"[..],
        }
    }

    pub(crate) fn macho_section_info(
        &self,
        section: StandardSection,
    ) -> (&'static [u8], &'static [u8], SectionKind, SectionFlags) {
        match section {
            StandardSection::Text => (
                &b"__TEXT"[..],
                &b"__text"[..],
                SectionKind::Text,
                SectionFlags::None,
            ),
            StandardSection::Data => (
                &b"__DATA"[..],
                &b"__data"[..],
                SectionKind::Data,
                SectionFlags::None,
            ),
            StandardSection::ReadOnlyData => (
                &b"__TEXT"[..],
                &b"__const"[..],
                SectionKind::ReadOnlyData,
                SectionFlags::None,
            ),
            StandardSection::ReadOnlyDataWithRel => (
                &b"__DATA"[..],
                &b"__const"[..],
                SectionKind::ReadOnlyDataWithRel,
                SectionFlags::None,
            ),
            StandardSection::ReadOnlyString => (
                &b"__TEXT"[..],
                &b"__cstring"[..],
                SectionKind::ReadOnlyString,
                SectionFlags::None,
            ),
            StandardSection::UninitializedData => (
                &b"__DATA"[..],
                &b"__bss"[..],
                SectionKind::UninitializedData,
                SectionFlags::None,
            ),
            StandardSection::Tls => (
                &b"__DATA"[..],
                &b"__thread_data"[..],
                SectionKind::Tls,
                SectionFlags::None,
            ),
            StandardSection::UninitializedTls => (
                &b"__DATA"[..],
                &b"__thread_bss"[..],
                SectionKind::UninitializedTls,
                SectionFlags::None,
            ),
            StandardSection::TlsVariables => (
                &b"__DATA"[..],
                &b"__thread_vars"[..],
                SectionKind::TlsVariables,
                SectionFlags::None,
            ),
            StandardSection::Common => (
                &b"__DATA"[..],
                &b"__common"[..],
                SectionKind::Common,
                SectionFlags::None,
            ),
            StandardSection::GnuProperty => {
                // Unsupported section.
                (&[], &[], SectionKind::Note, SectionFlags::None)
            }
        }
    }

    pub(crate) fn macho_section_flags(&self, section: &Section<'_>) -> SectionFlags {
        let flags = match section.kind {
            SectionKind::Text => macho::S_ATTR_PURE_INSTRUCTIONS | macho::S_ATTR_SOME_INSTRUCTIONS,
            SectionKind::Data => 0,
            SectionKind::ReadOnlyData | SectionKind::ReadOnlyDataWithRel => 0,
            SectionKind::ReadOnlyString => macho::S_CSTRING_LITERALS,
            SectionKind::UninitializedData | SectionKind::Common => macho::S_ZEROFILL,
            SectionKind::Tls => macho::S_THREAD_LOCAL_REGULAR,
            SectionKind::UninitializedTls => macho::S_THREAD_LOCAL_ZEROFILL,
            SectionKind::TlsVariables => macho::S_THREAD_LOCAL_VARIABLES,
            SectionKind::Debug | SectionKind::DebugString => macho::S_ATTR_DEBUG,
            SectionKind::OtherString => macho::S_CSTRING_LITERALS,
            SectionKind::Other | SectionKind::Linker | SectionKind::Metadata => 0,
            SectionKind::Note | SectionKind::Unknown | SectionKind::Elf(_) => {
                return SectionFlags::None;
            }
        };
        SectionFlags::MachO { flags }
    }

    pub(crate) fn macho_symbol_flags(&self, symbol: &Symbol) -> SymbolFlags<SectionId, SymbolId> {
        let mut n_desc = 0;
        if symbol.weak {
            if symbol.is_undefined() {
                n_desc |= macho::N_WEAK_REF;
            } else {
                n_desc |= macho::N_WEAK_DEF;
            }
        }
        // TODO: include n_type
        SymbolFlags::MachO { n_desc }
    }

    fn macho_tlv_bootstrap(&mut self) -> SymbolId {
        match self.tlv_bootstrap {
            Some(id) => id,
            None => {
                let id = self.add_symbol(Symbol {
                    name: b"_tlv_bootstrap".to_vec(),
                    value: 0,
                    size: 0,
                    kind: SymbolKind::Text,
                    scope: SymbolScope::Dynamic,
                    weak: false,
                    section: SymbolSection::Undefined,
                    flags: SymbolFlags::None,
                });
                self.tlv_bootstrap = Some(id);
                id
            }
        }
    }

    /// Create the `__thread_vars` entry for a TLS variable.
    ///
    /// The symbol given by `symbol_id` will be updated to point to this entry.
    ///
    /// A new `SymbolId` will be returned. The caller must update this symbol
    /// to point to the initializer.
    ///
    /// If `symbol_id` is not for a TLS variable, then it is returned unchanged.
    pub(crate) fn macho_add_thread_var(&mut self, symbol_id: SymbolId) -> SymbolId {
        let symbol = self.symbol_mut(symbol_id);
        if symbol.kind != SymbolKind::Tls {
            return symbol_id;
        }

        // Create the initializer symbol.
        let mut name = symbol.name.clone();
        name.extend_from_slice(b"$tlv$init");
        let init_symbol_id = self.add_raw_symbol(Symbol {
            name,
            value: 0,
            size: 0,
            kind: SymbolKind::Tls,
            scope: SymbolScope::Compilation,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });

        // Add the tlv entry.
        // Three pointers in size:
        //   - __tlv_bootstrap - used to make sure support exists
        //   - spare pointer - used when mapped by the runtime
        //   - pointer to symbol initializer
        let section = self.section_id(StandardSection::TlsVariables);
        let address_size = self.architecture.address_size().unwrap().bytes();
        let size = u64::from(address_size) * 3;
        let data = vec![0; size as usize];
        let offset = self.append_section_data(section, &data, u64::from(address_size));

        let tlv_bootstrap = self.macho_tlv_bootstrap();
        self.add_relocation(
            section,
            Relocation {
                offset,
                symbol: tlv_bootstrap,
                addend: 0,
                flags: RelocationFlags::Generic {
                    kind: RelocationKind::Absolute,
                    encoding: RelocationEncoding::Generic,
                    size: address_size * 8,
                },
            },
        )
        .unwrap();
        self.add_relocation(
            section,
            Relocation {
                offset: offset + u64::from(address_size) * 2,
                symbol: init_symbol_id,
                addend: 0,
                flags: RelocationFlags::Generic {
                    kind: RelocationKind::Absolute,
                    encoding: RelocationEncoding::Generic,
                    size: address_size * 8,
                },
            },
        )
        .unwrap();

        // Update the symbol to point to the tlv.
        let symbol = self.symbol_mut(symbol_id);
        symbol.value = offset;
        symbol.size = size;
        symbol.section = SymbolSection::Section(section);

        init_symbol_id
    }

    pub(crate) fn macho_translate_relocation(&mut self, reloc: &mut Relocation) -> Result<()> {
        use RelocationEncoding as E;
        use RelocationKind as K;

        let (kind, encoding, mut size) = if let RelocationFlags::Generic {
            kind,
            encoding,
            size,
        } = reloc.flags
        {
            (kind, encoding, size)
        } else {
            return Ok(());
        };
        // Aarch64 relocs of these sizes act as if they are double-word length
        if self.architecture == Architecture::Aarch64 && matches!(size, 12 | 21 | 26) {
            size = 32;
        }
        let r_length = match size {
            8 => 0,
            16 => 1,
            32 => 2,
            64 => 3,
            _ => return Err(Error(format!("unimplemented reloc size {:?}", reloc))),
        };
        let unsupported_reloc = || Err(Error(format!("unimplemented relocation {:?}", reloc)));
        let (r_pcrel, r_type) = match self.architecture {
            Architecture::I386 => match kind {
                K::Absolute => (false, macho::GENERIC_RELOC_VANILLA),
                _ => return unsupported_reloc(),
            },
            Architecture::Arm => match kind {
                K::Absolute => (false, macho::ARM_RELOC_VANILLA),
                _ => return unsupported_reloc(),
            },
            Architecture::X86_64 => match (kind, encoding) {
                (K::Absolute, E::Generic) => (false, macho::X86_64_RELOC_UNSIGNED),
                (K::Relative, E::Generic) => (true, macho::X86_64_RELOC_SIGNED),
                (K::Relative, E::X86RipRelative) => (true, macho::X86_64_RELOC_SIGNED),
                (K::Relative, E::X86Branch) => (true, macho::X86_64_RELOC_BRANCH),
                (K::PltRelative, E::X86Branch) => (true, macho::X86_64_RELOC_BRANCH),
                (K::GotRelative, E::Generic) => (true, macho::X86_64_RELOC_GOT),
                (K::GotRelative, E::X86RipRelativeMovq) => (true, macho::X86_64_RELOC_GOT_LOAD),
                _ => return unsupported_reloc(),
            },
            Architecture::Aarch64 | Architecture::Aarch64_Ilp32 => match (kind, encoding) {
                (K::Absolute, E::Generic) => (false, macho::ARM64_RELOC_UNSIGNED),
                (K::Relative, E::AArch64Call) => (true, macho::ARM64_RELOC_BRANCH26),
                _ => return unsupported_reloc(),
            },
            _ => {
                return Err(Error(format!(
                    "unimplemented architecture {:?}",
                    self.architecture
                )));
            }
        };
        reloc.flags = RelocationFlags::MachO {
            r_type,
            r_pcrel,
            r_length,
        };
        Ok(())
    }

    pub(crate) fn macho_adjust_addend(&mut self, relocation: &mut Relocation) -> Result<bool> {
        let (r_type, r_pcrel) = if let RelocationFlags::MachO {
            r_type, r_pcrel, ..
        } = relocation.flags
        {
            (r_type, r_pcrel)
        } else {
            return Err(Error(format!("invalid relocation flags {:?}", relocation)));
        };
        if r_pcrel {
            // For PC relative relocations on some architectures, the
            // addend does not include the offset required due to the
            // PC being different from the place of the relocation.
            // This differs from other file formats, so adjust the
            // addend here to account for this.
            let pcrel_offset = match self.architecture {
                Architecture::I386 => 4,
                Architecture::X86_64 => match r_type {
                    macho::X86_64_RELOC_SIGNED_1 => 5,
                    macho::X86_64_RELOC_SIGNED_2 => 6,
                    macho::X86_64_RELOC_SIGNED_4 => 8,
                    _ => 4,
                },
                // TODO: maybe missing support for some architectures and relocations
                _ => 0,
            };
            relocation.addend += pcrel_offset;
        }
        // Determine if addend is implicit.
        let implicit = if self.architecture == Architecture::Aarch64 {
            match r_type {
                macho::ARM64_RELOC_BRANCH26
                | macho::ARM64_RELOC_PAGE21
                | macho::ARM64_RELOC_PAGEOFF12 => false,
                _ => true,
            }
        } else {
            true
        };
        Ok(implicit)
    }

    pub(crate) fn macho_relocation_size(&self, reloc: &Relocation) -> Result<u8> {
        if let RelocationFlags::MachO { r_length, .. } = reloc.flags {
            Ok(8 << r_length)
        } else {
            Err(Error("invalid relocation flags".into()))
        }
    }

    pub(crate) fn macho_write(&self, buffer: &mut dyn WritableBuffer) -> Result<()> {
        let address_size = self.architecture.address_size().unwrap();
        let endian = self.endian;
        let macho32 = MachO32 { endian };
        let macho64 = MachO64 { endian };
        let macho: &dyn MachO = match address_size {
            AddressSize::U8 | AddressSize::U16 | AddressSize::U32 => &macho32,
            AddressSize::U64 => &macho64,
        };
        let pointer_align = address_size.bytes() as usize;

        // Calculate offsets of everything, and build strtab.
        let mut offset = 0;

        // Calculate size of Mach-O header.
        offset += macho.mach_header_size();

        // Calculate size of commands.
        let mut ncmds = 0;
        let command_offset = offset;

        // Calculate size of segment command and section headers.
        let segment_command_offset = offset;
        let segment_command_len =
            macho.segment_command_size() + self.sections.len() * macho.section_header_size();
        offset += segment_command_len;
        ncmds += 1;

        // Calculate size of build version.
        let build_version_offset = offset;
        if let Some(version) = &self.macho_build_version {
            offset += version.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of load dylinker command.
        let load_dylinker_offset = offset;
        if let Some(dylinker) = &self.macho_load_dylinker {
            offset += dylinker.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of entry point command.
        let entry_point_offset = offset;
        if let Some(entry_point) = &self.macho_entry_point {
            offset += entry_point.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of rpath commands.
        let mut rpath_offsets = Vec::with_capacity(self.macho_rpaths.len());
        for rpath in &self.macho_rpaths {
            rpath_offsets.push(offset);
            offset += rpath.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of id_dylib command.
        let id_dylib_offset = offset;
        if let Some(id_dylib) = &self.macho_id_dylib {
            offset += id_dylib.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of load_dylib commands.
        let mut load_dylib_offsets = Vec::with_capacity(self.macho_load_dylibs.len());
        for dylib in &self.macho_load_dylibs {
            load_dylib_offsets.push(offset);
            offset += dylib.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of UUID command.
        let uuid_offset = offset;
        if let Some(uuid) = &self.macho_uuid {
            offset += uuid.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of source version command.
        let source_version_offset = offset;
        if let Some(source_version) = &self.macho_source_version {
            offset += source_version.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of version min commands.
        let version_min_macosx_offset = offset;
        if let Some(version_min) = &self.macho_version_min_macosx {
            offset += version_min.cmdsize() as usize;
            ncmds += 1;
        }

        let version_min_iphoneos_offset = offset;
        if let Some(version_min) = &self.macho_version_min_iphoneos {
            offset += version_min.cmdsize() as usize;
            ncmds += 1;
        }

        let version_min_tvos_offset = offset;
        if let Some(version_min) = &self.macho_version_min_tvos {
            offset += version_min.cmdsize() as usize;
            ncmds += 1;
        }

        let version_min_watchos_offset = offset;
        if let Some(version_min) = &self.macho_version_min_watchos {
            offset += version_min.cmdsize() as usize;
            ncmds += 1;
        }

        // Calculate size of symtab command.
        let symtab_command_offset = offset;
        let symtab_command_len = mem::size_of::<macho::SymtabCommand<Endianness>>();
        offset += symtab_command_len;
        ncmds += 1;

        // Calculate size of dysymtab command.
        let dysymtab_command_offset = offset;
        let dysymtab_command_len = mem::size_of::<macho::DysymtabCommand<Endianness>>();
        offset += dysymtab_command_len;
        ncmds += 1;

        let sizeofcmds = offset - command_offset;

        // Calculate size of section data.
        // Section data can immediately follow the load commands without any alignment padding.
        let segment_file_offset = offset;
        let mut section_offsets = vec![SectionOffsets::default(); self.sections.len()];
        let mut address = 0;
        for (index, section) in self.sections.iter().enumerate() {
            section_offsets[index].index = 1 + index;
            if !section.is_bss() {
                address = align_u64(address, section.align);
                section_offsets[index].address = address;
                section_offsets[index].offset = segment_file_offset + address as usize;
                address += section.size;
            }
        }
        let segment_file_size = address as usize;
        offset += address as usize;
        for (index, section) in self.sections.iter().enumerate() {
            if section.is_bss() {
                debug_assert!(section.data.is_empty());
                address = align_u64(address, section.align);
                section_offsets[index].address = address;
                address += section.size;
            }
        }

        // Partition symbols and add symbol strings to strtab.
        let mut strtab = StringTable::default();
        let mut symbol_offsets = vec![SymbolOffsets::default(); self.symbols.len()];
        let mut local_symbols = vec![];
        let mut external_symbols = vec![];
        let mut undefined_symbols = vec![];
        for (index, symbol) in self.symbols.iter().enumerate() {
            // The unified API allows creating symbols that we don't emit, so filter
            // them out here.
            //
            // Since we don't actually emit the symbol kind, we validate it here too.
            match symbol.kind {
                SymbolKind::Text | SymbolKind::Data | SymbolKind::Tls | SymbolKind::Unknown => {}
                SymbolKind::File | SymbolKind::Section => continue,
                SymbolKind::Label => {
                    return Err(Error(format!(
                        "unimplemented symbol `{}` kind {:?}",
                        symbol.name().unwrap_or(""),
                        symbol.kind
                    )));
                }
            }
            if !symbol.name.is_empty() {
                symbol_offsets[index].str_id = Some(strtab.add(&symbol.name));
            }
            if symbol.is_undefined() {
                undefined_symbols.push(index);
            } else if symbol.is_local() {
                local_symbols.push(index);
            } else {
                external_symbols.push(index);
            }
        }

        external_symbols.sort_by_key(|index| &*self.symbols[*index].name);
        undefined_symbols.sort_by_key(|index| &*self.symbols[*index].name);

        // Count symbols.
        let mut nsyms = 0;
        for index in local_symbols
            .iter()
            .copied()
            .chain(external_symbols.iter().copied())
            .chain(undefined_symbols.iter().copied())
        {
            symbol_offsets[index].index = nsyms;
            nsyms += 1;
        }

        // Calculate size of relocations.
        for (index, section) in self.sections.iter().enumerate() {
            let count: usize = section
                .relocations
                .iter()
                .map(|reloc| 1 + usize::from(reloc.addend != 0))
                .sum();
            if count != 0 {
                offset = align(offset, pointer_align);
                section_offsets[index].reloc_offset = offset;
                section_offsets[index].reloc_count = count;
                let len = count * mem::size_of::<macho::Relocation<Endianness>>();
                offset += len;
            }
        }

        // Calculate size of symtab.
        offset = align(offset, pointer_align);
        let symtab_offset = offset;
        let symtab_len = nsyms * macho.nlist_size();
        offset += symtab_len;

        // Calculate size of strtab.
        let strtab_offset = offset;
        // Start with null name.
        let mut strtab_data = vec![0];
        strtab.write(1, &mut strtab_data);
        write_align(&mut strtab_data, pointer_align);
        offset += strtab_data.len();

        // Start writing.
        buffer
            .reserve(offset)
            .map_err(|_| Error(String::from("Cannot allocate buffer")))?;

        // Write file header.
        let (cputype, mut cpusubtype) = match (self.architecture, self.sub_architecture) {
            (Architecture::Arm, None) => (macho::CPU_TYPE_ARM, macho::CPU_SUBTYPE_ARM_ALL),
            (Architecture::Aarch64, None) => (macho::CPU_TYPE_ARM64, macho::CPU_SUBTYPE_ARM64_ALL),
            (Architecture::Aarch64, Some(SubArchitecture::Arm64E)) => {
                (macho::CPU_TYPE_ARM64, macho::CPU_SUBTYPE_ARM64E)
            }
            (Architecture::Aarch64_Ilp32, None) => {
                (macho::CPU_TYPE_ARM64_32, macho::CPU_SUBTYPE_ARM64_32_V8)
            }
            (Architecture::I386, None) => (macho::CPU_TYPE_X86, macho::CPU_SUBTYPE_I386_ALL),
            (Architecture::X86_64, None) => (macho::CPU_TYPE_X86_64, macho::CPU_SUBTYPE_X86_64_ALL),
            (Architecture::PowerPc, None) => {
                (macho::CPU_TYPE_POWERPC, macho::CPU_SUBTYPE_POWERPC_ALL)
            }
            (Architecture::PowerPc64, None) => {
                (macho::CPU_TYPE_POWERPC64, macho::CPU_SUBTYPE_POWERPC_ALL)
            }
            _ => {
                return Err(Error(format!(
                    "unimplemented architecture {:?} with sub-architecture {:?}",
                    self.architecture, self.sub_architecture
                )));
            }
        };

        if let Some(cpu_subtype) = self.macho_cpu_subtype {
            cpusubtype = cpu_subtype;
        }

        let mut flags = match self.flags {
            FileFlags::MachO { flags } => flags,
            _ => 0,
        };
        if self.macho_subsections_via_symbols {
            flags |= macho::MH_SUBSECTIONS_VIA_SYMBOLS;
        }
        macho.write_mach_header(
            buffer,
            MachHeader {
                cputype,
                cpusubtype,
                filetype: self.macho_file_type.unwrap_or(macho::MH_OBJECT),
                ncmds,
                sizeofcmds: sizeofcmds as u32,
                flags,
            },
        );

        // Write segment command.
        debug_assert_eq!(segment_command_offset, buffer.len());
        macho.write_segment_command(
            buffer,
            SegmentCommand {
                cmdsize: segment_command_len as u32,
                segname: [0; 16],
                vmaddr: 0,
                vmsize: address,
                fileoff: segment_file_offset as u64,
                filesize: segment_file_size as u64,
                maxprot: macho::VM_PROT_READ | macho::VM_PROT_WRITE | macho::VM_PROT_EXECUTE,
                initprot: macho::VM_PROT_READ | macho::VM_PROT_WRITE | macho::VM_PROT_EXECUTE,
                nsects: self.sections.len() as u32,
                flags: 0,
            },
        );

        // Write section headers.
        for (index, section) in self.sections.iter().enumerate() {
            let mut sectname = [0; 16];
            sectname
                .get_mut(..section.name.len())
                .ok_or_else(|| {
                    Error(format!(
                        "section name `{}` is too long",
                        section.name().unwrap_or(""),
                    ))
                })?
                .copy_from_slice(&section.name);
            let mut segname = [0; 16];
            segname
                .get_mut(..section.segment.len())
                .ok_or_else(|| {
                    Error(format!(
                        "segment name `{}` is too long",
                        section.segment().unwrap_or(""),
                    ))
                })?
                .copy_from_slice(&section.segment);
            let SectionFlags::MachO { flags } = self.section_flags(section) else {
                return Err(Error(format!(
                    "unimplemented section `{}` kind {:?}",
                    section.name().unwrap_or(""),
                    section.kind
                )));
            };
            macho.write_section(
                buffer,
                SectionHeader {
                    sectname,
                    segname,
                    addr: section_offsets[index].address,
                    size: section.size,
                    offset: section_offsets[index].offset as u32,
                    align: section.align.trailing_zeros(),
                    reloff: section_offsets[index].reloc_offset as u32,
                    nreloc: section_offsets[index].reloc_count as u32,
                    flags,
                },
            );
        }

        // Write build version.
        if let Some(version) = &self.macho_build_version {
            debug_assert_eq!(build_version_offset, buffer.len());
            buffer.write(&macho::BuildVersionCommand {
                cmd: U32::new(endian, macho::LC_BUILD_VERSION),
                cmdsize: U32::new(endian, version.cmdsize()),
                platform: U32::new(endian, version.platform),
                minos: U32::new(endian, version.minos),
                sdk: U32::new(endian, version.sdk),
                ntools: U32::new(endian, 0),
            });
        }

        // Write load dylinker command.
        if let Some(dylinker) = &self.macho_load_dylinker {
            debug_assert_eq!(load_dylinker_offset, buffer.len());
            let base_size = mem::size_of::<macho::DylinkerCommand<Endianness>>();
            buffer.write(&macho::DylinkerCommand {
                cmd: U32::new(endian, macho::LC_LOAD_DYLINKER),
                cmdsize: U32::new(endian, dylinker.cmdsize()),
                name: macho::LcStr {
                    offset: U32::new(endian, base_size as u32),
                },
            });
            // Write the dylinker path string (null-terminated)
            buffer.write_bytes(&dylinker.dylinker);
            buffer.write_bytes(&[0]); // null terminator
            // Pad to 8-byte alignment
            let written = base_size + dylinker.dylinker.len() + 1;
            let aligned = (written + 7) & !7;
            let padding = aligned - written;
            buffer.write_bytes(&vec![0; padding]);
        }

        // Write entry point command.
        if let Some(entry_point) = &self.macho_entry_point {
            debug_assert_eq!(entry_point_offset, buffer.len());
            buffer.write(&macho::EntryPointCommand {
                cmd: U32::new(endian, macho::LC_MAIN),
                cmdsize: U32::new(endian, entry_point.cmdsize()),
                entryoff: U64::new(endian, entry_point.entryoff),
                stacksize: U64::new(endian, entry_point.stacksize),
            });
        }

        // Write rpath commands.
        for (i, rpath) in self.macho_rpaths.iter().enumerate() {
            debug_assert_eq!(rpath_offsets[i], buffer.len());
            let base_size = mem::size_of::<macho::RpathCommand<Endianness>>();
            buffer.write(&macho::RpathCommand {
                cmd: U32::new(endian, macho::LC_RPATH),
                cmdsize: U32::new(endian, rpath.cmdsize()),
                path: macho::LcStr {
                    offset: U32::new(endian, base_size as u32),
                },
            });
            // Write the rpath string (null-terminated)
            buffer.write_bytes(&rpath.path);
            buffer.write_bytes(&[0]); // null terminator
            // Pad to 8-byte alignment
            let written = base_size + rpath.path.len() + 1;
            let aligned = (written + 7) & !7;
            let padding = aligned - written;
            buffer.write_bytes(&vec![0; padding]);
        }

        // Write id_dylib command.
        if let Some(id_dylib) = &self.macho_id_dylib {
            debug_assert_eq!(id_dylib_offset, buffer.len());
            let base_size = mem::size_of::<macho::DylibCommand<Endianness>>();
            buffer.write(&macho::DylibCommand {
                cmd: U32::new(endian, macho::LC_ID_DYLIB),
                cmdsize: U32::new(endian, id_dylib.cmdsize()),
                dylib: macho::Dylib {
                    name: macho::LcStr {
                        offset: U32::new(endian, base_size as u32),
                    },
                    timestamp: U32::new(endian, id_dylib.timestamp),
                    current_version: U32::new(endian, id_dylib.current_version),
                    compatibility_version: U32::new(endian, id_dylib.compatibility_version),
                },
            });
            // Write the dylib name string (null-terminated)
            buffer.write_bytes(&id_dylib.name);
            buffer.write_bytes(&[0]); // null terminator
            // Pad to 8-byte alignment
            let written = base_size + id_dylib.name.len() + 1;
            let aligned = (written + 7) & !7;
            let padding = aligned - written;
            buffer.write_bytes(&vec![0; padding]);
        }

        // Write load_dylib commands.
        for (i, dylib) in self.macho_load_dylibs.iter().enumerate() {
            debug_assert_eq!(load_dylib_offsets[i], buffer.len());
            let base_size = mem::size_of::<macho::DylibCommand<Endianness>>();
            buffer.write(&macho::DylibCommand {
                cmd: U32::new(endian, macho::LC_LOAD_DYLIB),
                cmdsize: U32::new(endian, dylib.cmdsize()),
                dylib: macho::Dylib {
                    name: macho::LcStr {
                        offset: U32::new(endian, base_size as u32),
                    },
                    timestamp: U32::new(endian, dylib.timestamp),
                    current_version: U32::new(endian, dylib.current_version),
                    compatibility_version: U32::new(endian, dylib.compatibility_version),
                },
            });
            // Write the dylib name string (null-terminated)
            buffer.write_bytes(&dylib.name);
            buffer.write_bytes(&[0]); // null terminator
            // Pad to 8-byte alignment
            let written = base_size + dylib.name.len() + 1;
            let aligned = (written + 7) & !7;
            let padding = aligned - written;
            buffer.write_bytes(&vec![0; padding]);
        }

        // Write UUID command.
        if let Some(uuid) = &self.macho_uuid {
            debug_assert_eq!(uuid_offset, buffer.len());
            buffer.write(&macho::UuidCommand {
                cmd: U32::new(endian, macho::LC_UUID),
                cmdsize: U32::new(endian, uuid.cmdsize()),
                uuid: uuid.uuid,
            });
        }

        // Write source version command.
        if let Some(source_version) = &self.macho_source_version {
            debug_assert_eq!(source_version_offset, buffer.len());
            buffer.write(&macho::SourceVersionCommand {
                cmd: U32::new(endian, macho::LC_SOURCE_VERSION),
                cmdsize: U32::new(endian, source_version.cmdsize()),
                version: U64::new(endian, source_version.version),
            });
        }

        // Write version min commands.
        if let Some(version_min) = &self.macho_version_min_macosx {
            debug_assert_eq!(version_min_macosx_offset, buffer.len());
            buffer.write(&macho::VersionMinCommand {
                cmd: U32::new(endian, macho::LC_VERSION_MIN_MACOSX),
                cmdsize: U32::new(endian, version_min.cmdsize()),
                version: U32::new(endian, version_min.version),
                sdk: U32::new(endian, version_min.sdk),
            });
        }

        if let Some(version_min) = &self.macho_version_min_iphoneos {
            debug_assert_eq!(version_min_iphoneos_offset, buffer.len());
            buffer.write(&macho::VersionMinCommand {
                cmd: U32::new(endian, macho::LC_VERSION_MIN_IPHONEOS),
                cmdsize: U32::new(endian, version_min.cmdsize()),
                version: U32::new(endian, version_min.version),
                sdk: U32::new(endian, version_min.sdk),
            });
        }

        if let Some(version_min) = &self.macho_version_min_tvos {
            debug_assert_eq!(version_min_tvos_offset, buffer.len());
            buffer.write(&macho::VersionMinCommand {
                cmd: U32::new(endian, macho::LC_VERSION_MIN_TVOS),
                cmdsize: U32::new(endian, version_min.cmdsize()),
                version: U32::new(endian, version_min.version),
                sdk: U32::new(endian, version_min.sdk),
            });
        }

        if let Some(version_min) = &self.macho_version_min_watchos {
            debug_assert_eq!(version_min_watchos_offset, buffer.len());
            buffer.write(&macho::VersionMinCommand {
                cmd: U32::new(endian, macho::LC_VERSION_MIN_WATCHOS),
                cmdsize: U32::new(endian, version_min.cmdsize()),
                version: U32::new(endian, version_min.version),
                sdk: U32::new(endian, version_min.sdk),
            });
        }

        // Write symtab command.
        debug_assert_eq!(symtab_command_offset, buffer.len());
        let symtab_command = macho::SymtabCommand {
            cmd: U32::new(endian, macho::LC_SYMTAB),
            cmdsize: U32::new(endian, symtab_command_len as u32),
            symoff: U32::new(endian, symtab_offset as u32),
            nsyms: U32::new(endian, nsyms as u32),
            stroff: U32::new(endian, strtab_offset as u32),
            strsize: U32::new(endian, strtab_data.len() as u32),
        };
        buffer.write(&symtab_command);

        // Write dysymtab command.
        debug_assert_eq!(dysymtab_command_offset, buffer.len());
        let dysymtab_command = macho::DysymtabCommand {
            cmd: U32::new(endian, macho::LC_DYSYMTAB),
            cmdsize: U32::new(endian, dysymtab_command_len as u32),
            ilocalsym: U32::new(endian, 0),
            nlocalsym: U32::new(endian, local_symbols.len() as u32),
            iextdefsym: U32::new(endian, local_symbols.len() as u32),
            nextdefsym: U32::new(endian, external_symbols.len() as u32),
            iundefsym: U32::new(
                endian,
                local_symbols.len() as u32 + external_symbols.len() as u32,
            ),
            nundefsym: U32::new(endian, undefined_symbols.len() as u32),
            tocoff: U32::default(),
            ntoc: U32::default(),
            modtaboff: U32::default(),
            nmodtab: U32::default(),
            extrefsymoff: U32::default(),
            nextrefsyms: U32::default(),
            indirectsymoff: U32::default(),
            nindirectsyms: U32::default(),
            extreloff: U32::default(),
            nextrel: U32::default(),
            locreloff: U32::default(),
            nlocrel: U32::default(),
        };
        buffer.write(&dysymtab_command);

        // Write section data.
        for (index, section) in self.sections.iter().enumerate() {
            if !section.is_bss() {
                buffer.resize(section_offsets[index].offset);
                buffer.write_bytes(&section.data);
            }
        }
        debug_assert_eq!(segment_file_offset + segment_file_size, buffer.len());

        // Write relocations.
        for (index, section) in self.sections.iter().enumerate() {
            if !section.relocations.is_empty() {
                write_align(buffer, pointer_align);
                debug_assert_eq!(section_offsets[index].reloc_offset, buffer.len());

                let mut write_reloc = |reloc: &Relocation| {
                    let (r_type, r_pcrel, r_length) = if let RelocationFlags::MachO {
                        r_type,
                        r_pcrel,
                        r_length,
                    } = reloc.flags
                    {
                        (r_type, r_pcrel, r_length)
                    } else {
                        return Err(Error("invalid relocation flags".into()));
                    };

                    // Write explicit addend.
                    if reloc.addend != 0 {
                        let r_type = match self.architecture {
                            Architecture::Aarch64 | Architecture::Aarch64_Ilp32 => {
                                macho::ARM64_RELOC_ADDEND
                            }
                            _ => {
                                return Err(Error(format!("unimplemented relocation {:?}", reloc)))
                            }
                        };

                        let reloc_info = macho::RelocationInfo {
                            r_address: reloc.offset as u32,
                            r_symbolnum: reloc.addend as u32,
                            r_pcrel: false,
                            r_length,
                            r_extern: false,
                            r_type,
                        };
                        buffer.write(&reloc_info.relocation(endian));
                    }

                    let r_extern;
                    let r_symbolnum;
                    let symbol = &self.symbols[reloc.symbol.0];
                    if symbol.kind == SymbolKind::Section {
                        r_symbolnum = section_offsets[symbol.section.id().unwrap().0].index as u32;
                        r_extern = false;
                    } else {
                        r_symbolnum = symbol_offsets[reloc.symbol.0].index as u32;
                        r_extern = true;
                    }

                    let reloc_info = macho::RelocationInfo {
                        r_address: reloc.offset as u32,
                        r_symbolnum,
                        r_pcrel,
                        r_length,
                        r_extern,
                        r_type,
                    };
                    buffer.write(&reloc_info.relocation(endian));
                    Ok(())
                };

                // Relocations are emitted in descending order as otherwise Apple's
                // new linker crashes. This matches LLVM's behavior too:
                // https://github.com/llvm/llvm-project/blob/e9b8cd0c8/llvm/lib/MC/MachObjectWriter.cpp#L1001-L1002
                let need_reverse = |relocs: &[Relocation]| {
                    let Some(first) = relocs.first() else {
                        return false;
                    };
                    let Some(last) = relocs.last() else {
                        return false;
                    };
                    first.offset < last.offset
                };
                if need_reverse(&section.relocations) {
                    for reloc in section.relocations.iter().rev() {
                        write_reloc(reloc)?;
                    }
                } else {
                    for reloc in &section.relocations {
                        write_reloc(reloc)?;
                    }
                }
            }
        }

        // Write symtab.
        write_align(buffer, pointer_align);
        debug_assert_eq!(symtab_offset, buffer.len());
        for index in local_symbols
            .iter()
            .copied()
            .chain(external_symbols.iter().copied())
            .chain(undefined_symbols.iter().copied())
        {
            let symbol = &self.symbols[index];
            // TODO: N_STAB
            let (mut n_type, n_sect) = match symbol.section {
                SymbolSection::Undefined => (macho::N_UNDF | macho::N_EXT, 0),
                SymbolSection::Absolute => (macho::N_ABS, 0),
                SymbolSection::Section(id) => (macho::N_SECT, id.0 + 1),
                SymbolSection::None | SymbolSection::Common => {
                    return Err(Error(format!(
                        "unimplemented symbol `{}` section {:?}",
                        symbol.name().unwrap_or(""),
                        symbol.section
                    )));
                }
            };
            match symbol.scope {
                SymbolScope::Unknown | SymbolScope::Compilation => {}
                SymbolScope::Linkage => {
                    n_type |= macho::N_EXT | macho::N_PEXT;
                }
                SymbolScope::Dynamic => {
                    n_type |= macho::N_EXT;
                }
            }

            let SymbolFlags::MachO { n_desc } = self.symbol_flags(symbol) else {
                return Err(Error(format!(
                    "unimplemented symbol `{}` kind {:?}",
                    symbol.name().unwrap_or(""),
                    symbol.kind
                )));
            };

            let n_value = match symbol.section.id() {
                Some(section) => section_offsets[section.0].address + symbol.value,
                None => symbol.value,
            };

            let n_strx = symbol_offsets[index]
                .str_id
                .map(|id| strtab.get_offset(id))
                .unwrap_or(0);

            macho.write_nlist(
                buffer,
                Nlist {
                    n_strx: n_strx as u32,
                    n_type,
                    n_sect: n_sect as u8,
                    n_desc,
                    n_value,
                },
            );
        }

        // Write strtab.
        debug_assert_eq!(strtab_offset, buffer.len());
        buffer.write_bytes(&strtab_data);

        debug_assert_eq!(offset, buffer.len());

        Ok(())
    }
}

struct MachHeader {
    cputype: u32,
    cpusubtype: u32,
    filetype: u32,
    ncmds: u32,
    sizeofcmds: u32,
    flags: u32,
}

struct SegmentCommand {
    cmdsize: u32,
    segname: [u8; 16],
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: u32,
    initprot: u32,
    nsects: u32,
    flags: u32,
}

pub struct SectionHeader {
    sectname: [u8; 16],
    segname: [u8; 16],
    addr: u64,
    size: u64,
    offset: u32,
    align: u32,
    reloff: u32,
    nreloc: u32,
    flags: u32,
}

struct Nlist {
    n_strx: u32,
    n_type: u8,
    n_sect: u8,
    n_desc: u16,
    n_value: u64,
}

trait MachO {
    fn mach_header_size(&self) -> usize;
    fn segment_command_size(&self) -> usize;
    fn section_header_size(&self) -> usize;
    fn nlist_size(&self) -> usize;
    fn write_mach_header(&self, buffer: &mut dyn WritableBuffer, section: MachHeader);
    fn write_segment_command(&self, buffer: &mut dyn WritableBuffer, segment: SegmentCommand);
    fn write_section(&self, buffer: &mut dyn WritableBuffer, section: SectionHeader);
    fn write_nlist(&self, buffer: &mut dyn WritableBuffer, nlist: Nlist);
}

struct MachO32<E> {
    endian: E,
}

impl<E: Endian> MachO for MachO32<E> {
    fn mach_header_size(&self) -> usize {
        mem::size_of::<macho::MachHeader32<E>>()
    }

    fn segment_command_size(&self) -> usize {
        mem::size_of::<macho::SegmentCommand32<E>>()
    }

    fn section_header_size(&self) -> usize {
        mem::size_of::<macho::Section32<E>>()
    }

    fn nlist_size(&self) -> usize {
        mem::size_of::<macho::Nlist32<E>>()
    }

    fn write_mach_header(&self, buffer: &mut dyn WritableBuffer, header: MachHeader) {
        let endian = self.endian;
        let magic = if endian.is_big_endian() {
            macho::MH_MAGIC
        } else {
            macho::MH_CIGAM
        };
        let header = macho::MachHeader32 {
            magic: U32::new(BigEndian, magic),
            cputype: U32::new(endian, header.cputype),
            cpusubtype: U32::new(endian, header.cpusubtype),
            filetype: U32::new(endian, header.filetype),
            ncmds: U32::new(endian, header.ncmds),
            sizeofcmds: U32::new(endian, header.sizeofcmds),
            flags: U32::new(endian, header.flags),
        };
        buffer.write(&header);
    }

    fn write_segment_command(&self, buffer: &mut dyn WritableBuffer, segment: SegmentCommand) {
        let endian = self.endian;
        let segment = macho::SegmentCommand32 {
            cmd: U32::new(endian, macho::LC_SEGMENT),
            cmdsize: U32::new(endian, segment.cmdsize),
            segname: segment.segname,
            vmaddr: U32::new(endian, segment.vmaddr as u32),
            vmsize: U32::new(endian, segment.vmsize as u32),
            fileoff: U32::new(endian, segment.fileoff as u32),
            filesize: U32::new(endian, segment.filesize as u32),
            maxprot: U32::new(endian, segment.maxprot),
            initprot: U32::new(endian, segment.initprot),
            nsects: U32::new(endian, segment.nsects),
            flags: U32::new(endian, segment.flags),
        };
        buffer.write(&segment);
    }

    fn write_section(&self, buffer: &mut dyn WritableBuffer, section: SectionHeader) {
        let endian = self.endian;
        let section = macho::Section32 {
            sectname: section.sectname,
            segname: section.segname,
            addr: U32::new(endian, section.addr as u32),
            size: U32::new(endian, section.size as u32),
            offset: U32::new(endian, section.offset),
            align: U32::new(endian, section.align),
            reloff: U32::new(endian, section.reloff),
            nreloc: U32::new(endian, section.nreloc),
            flags: U32::new(endian, section.flags),
            reserved1: U32::default(),
            reserved2: U32::default(),
        };
        buffer.write(&section);
    }

    fn write_nlist(&self, buffer: &mut dyn WritableBuffer, nlist: Nlist) {
        let endian = self.endian;
        let nlist = macho::Nlist32 {
            n_strx: U32::new(endian, nlist.n_strx),
            n_type: nlist.n_type,
            n_sect: nlist.n_sect,
            n_desc: U16::new(endian, nlist.n_desc),
            n_value: U32::new(endian, nlist.n_value as u32),
        };
        buffer.write(&nlist);
    }
}

struct MachO64<E> {
    endian: E,
}

impl<E: Endian> MachO for MachO64<E> {
    fn mach_header_size(&self) -> usize {
        mem::size_of::<macho::MachHeader64<E>>()
    }

    fn segment_command_size(&self) -> usize {
        mem::size_of::<macho::SegmentCommand64<E>>()
    }

    fn section_header_size(&self) -> usize {
        mem::size_of::<macho::Section64<E>>()
    }

    fn nlist_size(&self) -> usize {
        mem::size_of::<macho::Nlist64<E>>()
    }

    fn write_mach_header(&self, buffer: &mut dyn WritableBuffer, header: MachHeader) {
        let endian = self.endian;
        let magic = if endian.is_big_endian() {
            macho::MH_MAGIC_64
        } else {
            macho::MH_CIGAM_64
        };
        let header = macho::MachHeader64 {
            magic: U32::new(BigEndian, magic),
            cputype: U32::new(endian, header.cputype),
            cpusubtype: U32::new(endian, header.cpusubtype),
            filetype: U32::new(endian, header.filetype),
            ncmds: U32::new(endian, header.ncmds),
            sizeofcmds: U32::new(endian, header.sizeofcmds),
            flags: U32::new(endian, header.flags),
            reserved: U32::default(),
        };
        buffer.write(&header);
    }

    fn write_segment_command(&self, buffer: &mut dyn WritableBuffer, segment: SegmentCommand) {
        let endian = self.endian;
        let segment = macho::SegmentCommand64 {
            cmd: U32::new(endian, macho::LC_SEGMENT_64),
            cmdsize: U32::new(endian, segment.cmdsize),
            segname: segment.segname,
            vmaddr: U64::new(endian, segment.vmaddr),
            vmsize: U64::new(endian, segment.vmsize),
            fileoff: U64::new(endian, segment.fileoff),
            filesize: U64::new(endian, segment.filesize),
            maxprot: U32::new(endian, segment.maxprot),
            initprot: U32::new(endian, segment.initprot),
            nsects: U32::new(endian, segment.nsects),
            flags: U32::new(endian, segment.flags),
        };
        buffer.write(&segment);
    }

    fn write_section(&self, buffer: &mut dyn WritableBuffer, section: SectionHeader) {
        let endian = self.endian;
        let section = macho::Section64 {
            sectname: section.sectname,
            segname: section.segname,
            addr: U64::new(endian, section.addr),
            size: U64::new(endian, section.size),
            offset: U32::new(endian, section.offset),
            align: U32::new(endian, section.align),
            reloff: U32::new(endian, section.reloff),
            nreloc: U32::new(endian, section.nreloc),
            flags: U32::new(endian, section.flags),
            reserved1: U32::default(),
            reserved2: U32::default(),
            reserved3: U32::default(),
        };
        buffer.write(&section);
    }

    fn write_nlist(&self, buffer: &mut dyn WritableBuffer, nlist: Nlist) {
        let endian = self.endian;
        let nlist = macho::Nlist64 {
            n_strx: U32::new(endian, nlist.n_strx),
            n_type: nlist.n_type,
            n_sect: nlist.n_sect,
            n_desc: U16::new(endian, nlist.n_desc),
            n_value: U64Bytes::new(endian, nlist.n_value),
        };
        buffer.write(&nlist);
    }
}
