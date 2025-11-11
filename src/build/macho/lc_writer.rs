//! Load command writing utilities for Mach-O binaries.
//!
//! This module provides functions to calculate sizes and serialize load commands
//! for in-place binary modification.

use crate::macho;
use crate::Endianness;
use alloc::vec::Vec;

/// A writer for building load command sections.
///
/// This struct manages a buffer and handles endianness conversion
/// when writing load commands.
#[derive(Debug)]
pub struct LoadCommandWriter {
    buffer: Vec<u8>,
    endian: Endianness,
    cmd_count: u32,
}

impl LoadCommandWriter {
    /// Create a new load command writer with the given endianness.
    pub fn new(endian: Endianness) -> Self {
        Self {
            buffer: Vec::new(),
            endian,
            cmd_count: 0,
        }
    }

    /// Write a u32 value with the correct endianness.
    #[inline]
    fn write_u32(&mut self, val: u32) {
        let bytes = match self.endian {
            Endianness::Little => val.to_le_bytes(),
            Endianness::Big => val.to_be_bytes(),
        };
        self.buffer.extend_from_slice(&bytes);
    }

    /// Write a u64 value with the correct endianness.
    #[inline]
    fn write_u64(&mut self, val: u64) {
        let bytes = match self.endian {
            Endianness::Little => val.to_le_bytes(),
            Endianness::Big => val.to_be_bytes(),
        };
        self.buffer.extend_from_slice(&bytes);
    }

    /// Write raw bytes to the buffer.
    #[inline]
    fn write_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Pad the buffer to 8-byte alignment.
    #[inline]
    fn pad_to_alignment(&mut self) {
        while self.buffer.len() % 8 != 0 {
            self.buffer.push(0);
        }
    }

    /// Get the current buffer size.
    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get the number of commands written.
    #[inline]
    pub fn cmd_count(&self) -> u32 {
        self.cmd_count
    }

    /// Consume the writer and return the buffer and command count.
    pub fn into_bytes(self) -> (Vec<u8>, u32) {
        (self.buffer, self.cmd_count)
    }

    /// Write an LC_LOAD_DYLINKER command.
    pub fn write_dylinker(&mut self, path: &[u8]) {
        let cmdsize = calc_dylinker_size(path);

        self.write_u32(macho::LC_LOAD_DYLINKER);
        self.write_u32(cmdsize);
        self.write_u32(12); // offset to path string (always 12)
        self.write_bytes(path);
        self.buffer.push(0); // null terminator
        self.pad_to_alignment();

        self.cmd_count += 1;
    }

    /// Write an LC_MAIN command.
    pub fn write_main(&mut self, entryoff: u64, stacksize: u64) {
        self.write_u32(macho::LC_MAIN);
        self.write_u32(24); // cmdsize (fixed)
        self.write_u64(entryoff);
        self.write_u64(stacksize);

        self.cmd_count += 1;
    }

    /// Write an LC_RPATH command.
    pub fn write_rpath(&mut self, path: &[u8]) {
        let cmdsize = calc_rpath_size(path);
        self.write_rpath_with_size(path, cmdsize);
    }

    /// Write an LC_RPATH command with custom size (preserves padding).
    ///
    /// If the new path doesn't fit in the original cmdsize, the minimum required
    /// size will be used instead (this happens when the path grows).
    pub fn write_rpath_with_size(&mut self, path: &[u8], original_cmdsize: u32) {
        let min_size = calc_rpath_size(path);
        // Use the larger of original size or minimum required size
        let cmdsize = original_cmdsize.max(min_size);

        let start_pos = self.buffer.len();

        self.write_u32(macho::LC_RPATH);
        self.write_u32(cmdsize);
        self.write_u32(12); // offset to path string (always 12)
        self.write_bytes(path);
        self.buffer.push(0); // null terminator

        // Pad to the specified cmdsize (preserves original padding when possible)
        let current_size = self.buffer.len() - start_pos;
        let padding_needed = (cmdsize as usize).saturating_sub(current_size);
        for _ in 0..padding_needed {
            self.buffer.push(0);
        }

        self.cmd_count += 1;
    }

    /// Write an LC_ID_DYLIB command.
    pub fn write_id_dylib(&mut self, name: &[u8], timestamp: u32, current_version: u32, compatibility_version: u32) {
        let cmdsize = calc_dylib_size(name);
        self.write_id_dylib_with_size(name, timestamp, current_version, compatibility_version, cmdsize);
    }

    /// Write an LC_ID_DYLIB command with custom size (preserves padding).
    ///
    /// If the new name doesn't fit in the original cmdsize, the minimum required
    /// size will be used instead (this happens when the name grows).
    pub fn write_id_dylib_with_size(&mut self, name: &[u8], timestamp: u32, current_version: u32, compatibility_version: u32, original_cmdsize: u32) {
        let min_size = calc_dylib_size(name);
        // Use the larger of original size or minimum required size
        let cmdsize = original_cmdsize.max(min_size);

        let start_pos = self.buffer.len();

        self.write_u32(macho::LC_ID_DYLIB);
        self.write_u32(cmdsize);
        self.write_u32(24); // offset to name string (always 24)
        self.write_u32(timestamp);
        self.write_u32(current_version);
        self.write_u32(compatibility_version);
        self.write_bytes(name);
        self.buffer.push(0); // null terminator

        // Pad to the specified cmdsize (preserves original padding when possible)
        let current_size = self.buffer.len() - start_pos;
        let padding_needed = (cmdsize as usize).saturating_sub(current_size);
        for _ in 0..padding_needed {
            self.buffer.push(0);
        }

        self.cmd_count += 1;
    }

    /// Write an LC_LOAD_DYLIB command.
    pub fn write_load_dylib(&mut self, name: &[u8], timestamp: u32, current_version: u32, compatibility_version: u32) {
        let cmdsize = calc_dylib_size(name);
        self.write_load_dylib_with_size(macho::LC_LOAD_DYLIB, name, timestamp, current_version, compatibility_version, cmdsize);
    }

    /// Write an LC_LOAD_DYLIB, LC_LOAD_WEAK_DYLIB, or LC_REEXPORT_DYLIB command with custom size (preserves padding).
    ///
    /// If the new name doesn't fit in the original cmdsize, the minimum required
    /// size will be used instead (this happens when the name grows).
    pub fn write_load_dylib_with_size(&mut self, cmd: u32, name: &[u8], timestamp: u32, current_version: u32, compatibility_version: u32, original_cmdsize: u32) {
        let min_size = calc_dylib_size(name);
        // Use the larger of original size or minimum required size
        let cmdsize = original_cmdsize.max(min_size);

        let start_pos = self.buffer.len();

        self.write_u32(cmd);  // Use the provided command type (LC_LOAD_DYLIB, LC_LOAD_WEAK_DYLIB, or LC_REEXPORT_DYLIB)
        self.write_u32(cmdsize);
        self.write_u32(24); // offset to name string (always 24)
        self.write_u32(timestamp);
        self.write_u32(current_version);
        self.write_u32(compatibility_version);
        self.write_bytes(name);
        self.buffer.push(0); // null terminator

        // Pad to the specified cmdsize (preserves original padding when possible)
        let current_size = self.buffer.len() - start_pos;
        let padding_needed = (cmdsize as usize).saturating_sub(current_size);
        for _ in 0..padding_needed {
            self.buffer.push(0);
        }

        self.cmd_count += 1;
    }

    /// Write an LC_UUID command.
    pub fn write_uuid(&mut self, uuid: &[u8; 16]) {
        self.write_u32(macho::LC_UUID);
        self.write_u32(24); // cmdsize (fixed)
        self.write_bytes(uuid);

        self.cmd_count += 1;
    }

    /// Write an LC_SOURCE_VERSION command.
    pub fn write_source_version(&mut self, version: u64) {
        self.write_u32(macho::LC_SOURCE_VERSION);
        self.write_u32(16); // cmdsize (fixed)
        self.write_u64(version);

        self.cmd_count += 1;
    }

    /// Write an LC_BUILD_VERSION command.
    pub fn write_build_version(&mut self, platform: u32, minos: u32, sdk: u32) {
        self.write_u32(macho::LC_BUILD_VERSION);
        self.write_u32(24); // cmdsize (no build tools for now)
        self.write_u32(platform);
        self.write_u32(minos);
        self.write_u32(sdk);
        self.write_u32(0); // ntools

        self.cmd_count += 1;
    }

    /// Write a version min command (LC_VERSION_MIN_MACOSX, etc.).
    pub fn write_version_min(&mut self, cmd: u32, version: u32, sdk: u32) {
        self.write_u32(cmd);
        self.write_u32(16); // cmdsize (fixed)
        self.write_u32(version);
        self.write_u32(sdk);

        self.cmd_count += 1;
    }

    /// Write a raw/unknown command.
    ///
    /// The data should include the complete load command (cmd, cmdsize, and all fields).
    /// The data is assumed to already be properly aligned, so no additional padding is added.
    pub fn write_raw_command(&mut self, data: &[u8]) {
        self.write_bytes(data);
        // Raw commands are assumed to already be properly aligned from the original binary
        // No additional padding needed

        self.cmd_count += 1;
    }
}

/// Align a size to the next 8-byte boundary.
///
/// Load commands must be aligned to 8 bytes in Mach-O files.
#[inline]
pub fn align_load_command_size(size: usize) -> usize {
    (size + 7) & !7
}

/// Calculate the size of an LC_LOAD_DYLINKER command.
///
/// Structure:
/// - 4 bytes: cmd (LC_LOAD_DYLINKER)
/// - 4 bytes: cmdsize
/// - 4 bytes: offset to dylinker path (always 12)
/// - N bytes: dylinker path string
/// - 1 byte: null terminator
/// - padding to 8-byte alignment
pub fn calc_dylinker_size(path: &[u8]) -> u32 {
    let base_size = 12; // cmd + cmdsize + offset
    let string_size = path.len() + 1; // +1 for null terminator
    let total = base_size + string_size;
    align_load_command_size(total) as u32
}

/// Calculate the size of an LC_MAIN command.
///
/// Structure:
/// - 4 bytes: cmd (LC_MAIN)
/// - 4 bytes: cmdsize (always 24)
/// - 8 bytes: entryoff
/// - 8 bytes: stacksize
///
/// This is a fixed-size command.
pub fn calc_main_size() -> u32 {
    24
}

/// Calculate the size of an LC_RPATH command.
///
/// Structure:
/// - 4 bytes: cmd (LC_RPATH)
/// - 4 bytes: cmdsize
/// - 4 bytes: offset to path (always 12)
/// - N bytes: rpath string
/// - 1 byte: null terminator
/// - padding to 8-byte alignment
pub fn calc_rpath_size(path: &[u8]) -> u32 {
    let base_size = 12; // cmd + cmdsize + offset
    let string_size = path.len() + 1; // +1 for null terminator
    let total = base_size + string_size;
    align_load_command_size(total) as u32
}

/// Calculate the size of an LC_ID_DYLIB or LC_LOAD_DYLIB command.
///
/// Structure:
/// - 4 bytes: cmd (LC_ID_DYLIB or LC_LOAD_DYLIB)
/// - 4 bytes: cmdsize
/// - 4 bytes: offset to name (always 24)
/// - 4 bytes: timestamp
/// - 4 bytes: current_version
/// - 4 bytes: compatibility_version
/// - N bytes: dylib name string
/// - 1 byte: null terminator
/// - padding to 8-byte alignment
pub fn calc_dylib_size(name: &[u8]) -> u32 {
    let base_size = 24; // cmd + cmdsize + offset + timestamp + 2 versions
    let string_size = name.len() + 1; // +1 for null terminator
    let total = base_size + string_size;
    align_load_command_size(total) as u32
}

/// Calculate the size of an LC_UUID command.
///
/// Structure:
/// - 4 bytes: cmd (LC_UUID)
/// - 4 bytes: cmdsize (always 24)
/// - 16 bytes: UUID
///
/// This is a fixed-size command.
pub fn calc_uuid_size() -> u32 {
    24
}

/// Calculate the size of an LC_SOURCE_VERSION command.
///
/// Structure:
/// - 4 bytes: cmd (LC_SOURCE_VERSION)
/// - 4 bytes: cmdsize (always 16)
/// - 8 bytes: version (A.B.C.D.E packed)
///
/// This is a fixed-size command.
pub fn calc_source_version_size() -> u32 {
    16
}

/// Calculate the size of an LC_BUILD_VERSION command.
///
/// Structure:
/// - 4 bytes: cmd (LC_BUILD_VERSION)
/// - 4 bytes: cmdsize
/// - 4 bytes: platform
/// - 4 bytes: minos
/// - 4 bytes: sdk
/// - 4 bytes: ntools
/// - N * 8 bytes: build tool entries (version + tool)
///
/// For now, we don't support build tools, so ntools=0.
pub fn calc_build_version_size(_ntools: u32) -> u32 {
    // Base size with ntools=0
    24 // cmd + cmdsize + platform + minos + sdk + ntools
    // TODO: Add build tool support: + (ntools * 8)
}

/// Calculate the size of a version min command.
///
/// Structure:
/// - 4 bytes: cmd (LC_VERSION_MIN_MACOSX, etc.)
/// - 4 bytes: cmdsize (always 16)
/// - 4 bytes: version
/// - 4 bytes: sdk
///
/// This is a fixed-size command.
pub fn calc_version_min_size() -> u32 {
    16
}

/// Calculate the size of a raw/unknown command.
///
/// The data should already include the cmd and cmdsize fields.
/// We just need to ensure it's properly aligned.
pub fn calc_raw_command_size(data: &[u8]) -> u32 {
    // Data should already be aligned, but verify
    align_load_command_size(data.len()) as u32
}

/// Calculate the total size of all load commands in a Builder.
///
/// This is used to determine if modified load commands will fit in the
/// original space, or if we need to relocate segments.
///
/// Returns a tuple of (total_size, command_count).
///
/// NOTE: This function is currently not used since we changed to an ordered
/// command structure. Instead, we build the commands and check the buffer size.
/// Keeping this for reference/future use.
#[allow(dead_code)]
pub fn calc_total_load_commands_size(
    load_commands: &super::LoadCommands<'_>,
) -> (u32, u32) {
    let mut total_size = 0u32;
    let mut cmd_count = 0u32;

    for command in &load_commands.commands {
        match command {
            super::LoadCommand::Rpath(rpath) => {
                total_size += calc_rpath_size(&rpath.rpath.path);
                cmd_count += 1;
            }
            super::LoadCommand::LoadDylib(dylib) => {
                total_size += calc_dylib_size(&dylib.dylib.name);
                cmd_count += 1;
            }
            super::LoadCommand::IdDylib(dylib) => {
                total_size += calc_dylib_size(&dylib.dylib.name);
                cmd_count += 1;
            }
            super::LoadCommand::Raw { cmd: _, data } => {
                total_size += calc_raw_command_size(data);
                cmd_count += 1;
            }
        }
    }

    (total_size, cmd_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_load_command_size() {
        assert_eq!(align_load_command_size(0), 0);
        assert_eq!(align_load_command_size(1), 8);
        assert_eq!(align_load_command_size(7), 8);
        assert_eq!(align_load_command_size(8), 8);
        assert_eq!(align_load_command_size(9), 16);
        assert_eq!(align_load_command_size(15), 16);
        assert_eq!(align_load_command_size(16), 16);
        assert_eq!(align_load_command_size(24), 24);
        assert_eq!(align_load_command_size(25), 32);
    }

    #[test]
    fn test_calc_dylinker_size() {
        // "/usr/lib/dyld" = 13 bytes + 1 null = 14
        // 12 (header) + 14 = 26, aligned to 32
        assert_eq!(calc_dylinker_size(b"/usr/lib/dyld"), 32);

        // Short path "a" = 1 + 1 = 2
        // 12 + 2 = 14, aligned to 16
        assert_eq!(calc_dylinker_size(b"a"), 16);

        // Empty path = 0 + 1 = 1
        // 12 + 1 = 13, aligned to 16
        assert_eq!(calc_dylinker_size(b""), 16);
    }

    #[test]
    fn test_calc_main_size() {
        assert_eq!(calc_main_size(), 24);
    }

    #[test]
    fn test_calc_rpath_size() {
        // "@executable_path/../Frameworks" = 31 bytes + 1 null = 32
        // 12 (header) + 32 = 44, aligned to 48
        assert_eq!(calc_rpath_size(b"@executable_path/../Frameworks"), 48);

        // "@rpath" = 6 + 1 = 7
        // 12 + 7 = 19, aligned to 24
        assert_eq!(calc_rpath_size(b"@rpath"), 24);
    }

    #[test]
    fn test_calc_dylib_size() {
        // "/usr/lib/libSystem.B.dylib" = 26 bytes + 1 null = 27
        // 24 (header) + 27 = 51, aligned to 56
        assert_eq!(calc_dylib_size(b"/usr/lib/libSystem.B.dylib"), 56);

        // "a.dylib" = 7 + 1 = 8
        // 24 + 8 = 32, aligned to 32
        assert_eq!(calc_dylib_size(b"a.dylib"), 32);
    }

    #[test]
    fn test_fixed_size_commands() {
        assert_eq!(calc_uuid_size(), 24);
        assert_eq!(calc_source_version_size(), 16);
        assert_eq!(calc_build_version_size(0), 24);
        assert_eq!(calc_version_min_size(), 16);
    }

    #[test]
    fn test_calc_raw_command_size() {
        // Already aligned
        assert_eq!(calc_raw_command_size(&[0u8; 16]), 16);
        assert_eq!(calc_raw_command_size(&[0u8; 24]), 24);

        // Needs alignment
        assert_eq!(calc_raw_command_size(&[0u8; 13]), 16);
        assert_eq!(calc_raw_command_size(&[0u8; 25]), 32);
    }

    // Tests for LoadCommandWriter serialization

    #[test]
    fn test_write_rpath_little_endian() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_rpath(b"@rpath");
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 24); // Should match calc_rpath_size

        // Verify cmd field (LC_RPATH = 0x8000001c)
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_RPATH
        );

        // Verify cmdsize field
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            24
        );

        // Verify offset to path string (always 12)
        assert_eq!(
            u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
            12
        );

        // Verify path string and null terminator
        assert_eq!(&buffer[12..18], b"@rpath");
        assert_eq!(buffer[18], 0); // null terminator

        // Verify padding (should be zeros to reach 24 bytes)
        for i in 19..24 {
            assert_eq!(buffer[i], 0);
        }
    }

    #[test]
    fn test_write_rpath_big_endian() {
        let mut writer = LoadCommandWriter::new(Endianness::Big);
        writer.write_rpath(b"@rpath");
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 24);

        // Verify cmd field (big-endian)
        assert_eq!(
            u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_RPATH
        );

        // Verify cmdsize field (big-endian)
        assert_eq!(
            u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            24
        );
    }

    #[test]
    fn test_write_main() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_main(0x1000, 0x2000);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 24); // Fixed size

        // Verify cmd field (LC_MAIN = 0x80000028)
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_MAIN
        );

        // Verify cmdsize
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            24
        );

        // Verify entryoff (u64)
        assert_eq!(
            u64::from_le_bytes([
                buffer[8], buffer[9], buffer[10], buffer[11], buffer[12], buffer[13], buffer[14],
                buffer[15]
            ]),
            0x1000
        );

        // Verify stacksize (u64)
        assert_eq!(
            u64::from_le_bytes([
                buffer[16], buffer[17], buffer[18], buffer[19], buffer[20], buffer[21],
                buffer[22], buffer[23]
            ]),
            0x2000
        );
    }

    #[test]
    fn test_write_dylinker() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_dylinker(b"/usr/lib/dyld");
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 32); // calc_dylinker_size(b"/usr/lib/dyld")

        // Verify cmd field
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_LOAD_DYLINKER
        );

        // Verify cmdsize
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            32
        );

        // Verify path string
        assert_eq!(&buffer[12..25], b"/usr/lib/dyld");
        assert_eq!(buffer[25], 0); // null terminator
    }

    #[test]
    fn test_write_load_dylib() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_load_dylib(b"/usr/lib/libSystem.B.dylib", 2, 0x10000, 0x10000);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 56); // calc_dylib_size(b"/usr/lib/libSystem.B.dylib")

        // Verify cmd field (LC_LOAD_DYLIB = 0x0c)
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_LOAD_DYLIB
        );

        // Verify cmdsize
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            56
        );

        // Verify offset to name (always 24)
        assert_eq!(
            u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
            24
        );

        // Verify timestamp
        assert_eq!(
            u32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]),
            2
        );

        // Verify current_version
        assert_eq!(
            u32::from_le_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]),
            0x10000
        );

        // Verify compatibility_version
        assert_eq!(
            u32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]),
            0x10000
        );

        // Verify name string
        assert_eq!(&buffer[24..50], b"/usr/lib/libSystem.B.dylib");
        assert_eq!(buffer[50], 0); // null terminator
    }

    #[test]
    fn test_write_id_dylib() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_id_dylib(b"@rpath/MyLib.dylib", 1, 0x10000, 0x10000);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 48); // calc_dylib_size(b"@rpath/MyLib.dylib")

        // Verify cmd field (LC_ID_DYLIB = 0x0d)
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_ID_DYLIB
        );

        // Verify cmdsize
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            48
        );
    }

    #[test]
    fn test_write_uuid() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        let uuid = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ];
        writer.write_uuid(&uuid);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 24); // Fixed size

        // Verify cmd field (LC_UUID = 0x1b)
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_UUID
        );

        // Verify cmdsize
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            24
        );

        // Verify UUID bytes
        assert_eq!(&buffer[8..24], &uuid);
    }

    #[test]
    fn test_write_source_version() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_source_version(0x0102030405);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 16); // Fixed size

        // Verify cmd field (LC_SOURCE_VERSION = 0x2a)
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_SOURCE_VERSION
        );

        // Verify cmdsize
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            16
        );

        // Verify version
        assert_eq!(
            u64::from_le_bytes([
                buffer[8], buffer[9], buffer[10], buffer[11], buffer[12], buffer[13], buffer[14],
                buffer[15]
            ]),
            0x0102030405
        );
    }

    #[test]
    fn test_write_build_version() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_build_version(1, 0xa0000, 0xb0000);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 24); // Fixed size for ntools=0

        // Verify cmd field (LC_BUILD_VERSION = 0x32)
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_BUILD_VERSION
        );

        // Verify cmdsize
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            24
        );

        // Verify platform
        assert_eq!(
            u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
            1
        );

        // Verify minos
        assert_eq!(
            u32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]),
            0xa0000
        );

        // Verify sdk
        assert_eq!(
            u32::from_le_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]),
            0xb0000
        );

        // Verify ntools
        assert_eq!(
            u32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]),
            0
        );
    }

    #[test]
    fn test_write_version_min() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_version_min(macho::LC_VERSION_MIN_MACOSX, 0xa0000, 0xb0000);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 16); // Fixed size

        // Verify cmd field
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_VERSION_MIN_MACOSX
        );

        // Verify cmdsize
        assert_eq!(
            u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            16
        );

        // Verify version
        assert_eq!(
            u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
            0xa0000
        );

        // Verify sdk
        assert_eq!(
            u32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]),
            0xb0000
        );
    }

    #[test]
    fn test_write_raw_command() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        // Create a fake raw command (must already include cmd, cmdsize, and be aligned)
        let raw_data = vec![0x99, 0x88, 0x77, 0x66, 0x10, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0x00, 0x00, 0x00, 0x00];
        writer.write_raw_command(&raw_data);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 16); // Already aligned

        // Verify data is copied verbatim
        assert_eq!(&buffer[..], &raw_data[..]);
    }

    #[test]
    fn test_write_raw_command_no_padding() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        // Raw command data - assumed to already include any necessary padding
        // This would be 13 bytes, but raw commands are assumed pre-aligned
        let raw_data = vec![0x01, 0x02, 0x03, 0x04, 0x10, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00, 0x00, 0x00];
        writer.write_raw_command(&raw_data);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 1);
        assert_eq!(buffer.len(), 16); // No additional padding added

        // Verify data is copied verbatim
        assert_eq!(&buffer[..], &raw_data[..]);
    }

    #[test]
    fn test_multiple_commands() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);

        // Write multiple commands
        writer.write_rpath(b"@rpath");
        writer.write_main(0x1000, 0);
        writer.write_uuid(&[0; 16]);

        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 3);
        // 24 (rpath) + 24 (main) + 24 (uuid) = 72
        assert_eq!(buffer.len(), 72);

        // Verify first command is rpath
        assert_eq!(
            u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            macho::LC_RPATH
        );

        // Verify second command is main (starts at offset 24)
        assert_eq!(
            u32::from_le_bytes([buffer[24], buffer[25], buffer[26], buffer[27]]),
            macho::LC_MAIN
        );

        // Verify third command is uuid (starts at offset 48)
        assert_eq!(
            u32::from_le_bytes([buffer[48], buffer[49], buffer[50], buffer[51]]),
            macho::LC_UUID
        );
    }

    #[test]
    fn test_writer_empty() {
        let writer = LoadCommandWriter::new(Endianness::Little);
        let (buffer, count) = writer.into_bytes();

        assert_eq!(count, 0);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_alignment_edge_cases() {
        let mut writer = LoadCommandWriter::new(Endianness::Little);

        // Test with paths of various lengths to ensure alignment works
        // Path length 1: 12 + 1 + 1 = 14 -> aligned to 16
        writer.write_rpath(b"a");
        let (buffer, _) = writer.into_bytes();
        assert_eq!(buffer.len(), 16);
        assert_eq!(buffer.len() % 8, 0);

        // Path length 6: 12 + 6 + 1 = 19 -> aligned to 24
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_rpath(b"abcdef");
        let (buffer, _) = writer.into_bytes();
        assert_eq!(buffer.len(), 24);
        assert_eq!(buffer.len() % 8, 0);

        // Path length 11: 12 + 11 + 1 = 24 -> already aligned
        let mut writer = LoadCommandWriter::new(Endianness::Little);
        writer.write_rpath(b"abcdefghijk");
        let (buffer, _) = writer.into_bytes();
        assert_eq!(buffer.len(), 24);
        assert_eq!(buffer.len() % 8, 0);
    }
}
