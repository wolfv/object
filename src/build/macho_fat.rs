//! Support for building Mach-O fat/universal binaries.

use crate::endian::{BigEndian, U32, U64};
use crate::macho;
use crate::pod::bytes_of;
use crate::read::{self, ReadRef};

use super::macho::Builder;
use alloc::vec::Vec;
use core::marker::PhantomData;

/// A builder for Mach-O fat/universal binaries containing multiple architectures.
///
/// This allows reading, modifying, and writing fat binaries that contain multiple
/// architecture slices (e.g., x86_64 and arm64 in a single file).
#[derive(Debug)]
pub struct FatBuilder<'data> {
    /// The individual architecture slices with their alignment (as power of 2).
    /// The alignment value is preserved from the original fat binary for byte-for-byte compatibility.
    pub slices: Vec<(Builder<'data>, u32)>,
    /// Whether this is a 64-bit fat binary (true) or 32-bit (false).
    is_64bit: bool,
    marker: PhantomData<&'data ()>,
}

impl<'data> FatBuilder<'data> {
    /// Get default alignment for a given CPU type.
    ///
    /// Returns alignment as power of 2 (e.g., 12 = 4KB, 14 = 16KB).
    /// This matches Apple's typical alignment choices for different architectures.
    fn default_alignment_for_cpu(cpu_type: u32) -> u32 {
        match cpu_type {
            // x86 and x86_64 typically use 4KB (2^12) alignment
            macho::CPU_TYPE_X86 | macho::CPU_TYPE_X86_64 => 12,
            // ARM64 uses 16KB (2^14) alignment (matching page size on Apple Silicon)
            macho::CPU_TYPE_ARM64 | macho::CPU_TYPE_ARM64_32 => 14,
            // Default to 14 (16KB) for other architectures as a safe choice
            _ => 14,
        }
    }

    /// Read a fat/universal Mach-O binary from data.
    ///
    /// This detects whether the file is a 32-bit or 64-bit fat binary
    /// and parses all architecture slices.
    pub fn read<R: ReadRef<'data>>(data: R) -> crate::build::Result<Self> {
        // Try to parse as 32-bit fat binary first
        if let Ok(fat32) = read::macho::MachOFatFile32::parse(data) {
            return Self::read_fat32(data, fat32);
        }

        // Try 64-bit fat binary
        if let Ok(fat64) = read::macho::MachOFatFile64::parse(data) {
            return Self::read_fat64(data, fat64);
        }

        Err(crate::build::Error::new("Not a fat Mach-O file"))
    }

    fn read_fat32<R: ReadRef<'data>>(
        data: R,
        fat: read::macho::MachOFatFile32<'data>,
    ) -> crate::build::Result<Self> {
        use read::macho::FatArch;

        let mut slices = Vec::new();

        for arch in fat.arches() {
            let arch_data = arch
                .data(data)
                .map_err(|e| crate::build::Error::new(format!("Failed to read fat arch: {}", e)))?;
            let builder = Builder::read(arch_data)?;

            // Preserve the original alignment from the fat binary
            let align = arch.align();

            slices.push((builder, align));
        }

        Ok(FatBuilder {
            slices,
            is_64bit: false,
            marker: PhantomData,
        })
    }

    fn read_fat64<R: ReadRef<'data>>(
        data: R,
        fat: read::macho::MachOFatFile64<'data>,
    ) -> crate::build::Result<Self> {
        use read::macho::FatArch;

        let mut slices = Vec::new();

        for arch in fat.arches() {
            let arch_data = arch
                .data(data)
                .map_err(|e| crate::build::Error::new(format!("Failed to read fat arch: {}", e)))?;
            let builder = Builder::read(arch_data)?;

            // Preserve the original alignment from the fat binary
            let align = arch.align();

            slices.push((builder, align));
        }

        Ok(FatBuilder {
            slices,
            is_64bit: true,
            marker: PhantomData,
        })
    }

    /// Write the fat binary to a buffer.
    ///
    /// This writes all architecture slices and creates a proper fat binary header.
    pub fn write(self) -> crate::build::Result<Vec<u8>> {
        if self.slices.is_empty() {
            return Err(crate::build::Error::new("No slices to write"));
        }

        // Collect CPU info and alignments before consuming slices
        let cpu_infos: Vec<(u32, u32)> = self.slices.iter().map(|(s, _)| get_cpu_info(s)).collect();
        let alignments: Vec<u32> = self.slices.iter().map(|(_, align)| *align).collect();

        // Write each slice
        let mut slice_data = Vec::new();
        for (slice, _align) in self.slices {
            let data = slice.write()?;
            slice_data.push(data);
        }

        // Calculate offsets using preserved alignments
        let header_size = core::mem::size_of::<macho::FatHeader>();
        let arch_size = if self.is_64bit {
            core::mem::size_of::<macho::FatArch64>()
        } else {
            core::mem::size_of::<macho::FatArch32>()
        };
        let mut offset = (header_size + arch_size * slice_data.len()) as u64;

        let mut arches = Vec::new();
        for (i, data) in slice_data.iter().enumerate() {
            // Use preserved alignment for each slice
            let align_bits = alignments[i];
            let align_size = 1u64 << align_bits;

            // Align to this slice's alignment
            offset = (offset + align_size - 1) & !(align_size - 1);

            arches.push((offset, data.len() as u64, align_bits));
            offset += data.len() as u64;
        }

        // Build the output
        let mut buffer = Vec::new();

        if self.is_64bit {
            // Write 64-bit fat header
            let header = macho::FatHeader {
                magic: U32::new(BigEndian, macho::FAT_MAGIC_64),
                nfat_arch: U32::new(BigEndian, slice_data.len() as u32),
            };
            buffer.extend_from_slice(bytes_of(&header));

            // Write 64-bit arch headers
            for (i, (offset, size, align_bits)) in arches.iter().enumerate() {
                let (cputype, cpusubtype) = cpu_infos[i];

                let arch = macho::FatArch64 {
                    cputype: U32::new(BigEndian, cputype),
                    cpusubtype: U32::new(BigEndian, cpusubtype),
                    offset: U64::new(BigEndian, *offset),
                    size: U64::new(BigEndian, *size),
                    align: U32::new(BigEndian, *align_bits),
                    reserved: U32::new(BigEndian, 0),
                };
                buffer.extend_from_slice(bytes_of(&arch));
            }
        } else {
            // Write 32-bit fat header
            let header = macho::FatHeader {
                magic: U32::new(BigEndian, macho::FAT_MAGIC),
                nfat_arch: U32::new(BigEndian, slice_data.len() as u32),
            };
            buffer.extend_from_slice(bytes_of(&header));

            // Write 32-bit arch headers
            for (i, (offset, size, align_bits)) in arches.iter().enumerate() {
                let (cputype, cpusubtype) = cpu_infos[i];

                let arch = macho::FatArch32 {
                    cputype: U32::new(BigEndian, cputype),
                    cpusubtype: U32::new(BigEndian, cpusubtype),
                    offset: U32::new(BigEndian, *offset as u32),
                    size: U32::new(BigEndian, *size as u32),
                    align: U32::new(BigEndian, *align_bits),
                };
                buffer.extend_from_slice(bytes_of(&arch));
            }
        }

        // Pad to first slice offset
        while buffer.len() < arches[0].0 as usize {
            buffer.push(0);
        }

        // Write slice data with padding
        for (i, data) in slice_data.iter().enumerate() {
            buffer.extend_from_slice(data);

            // Pad to next slice (except for last one)
            if i + 1 < arches.len() {
                while buffer.len() < arches[i + 1].0 as usize {
                    buffer.push(0);
                }
            }
        }

        Ok(buffer)
    }

    /// Get the number of architecture slices.
    pub fn len(&self) -> usize {
        self.slices.len()
    }

    /// Check if there are no slices.
    pub fn is_empty(&self) -> bool {
        self.slices.is_empty()
    }

    /// Get a specific architecture slice by index.
    pub fn get_slice(&self, index: usize) -> Option<&Builder<'data>> {
        self.slices.get(index).map(|(builder, _)| builder)
    }

    /// Get a mutable reference to a specific architecture slice by index.
    pub fn get_slice_mut(&mut self, index: usize) -> Option<&mut Builder<'data>> {
        self.slices.get_mut(index).map(|(builder, _)| builder)
    }

    /// Iterate over all slices.
    pub fn iter(&self) -> impl Iterator<Item = &Builder<'data>> {
        self.slices.iter().map(|(builder, _)| builder)
    }

    /// Iterate mutably over all slices.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Builder<'data>> {
        self.slices.iter_mut().map(|(builder, _)| builder)
    }

    /// Apply a function to all slices.
    ///
    /// This is useful for modifying all architectures in the same way.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fat_builder.for_each_slice(|builder| {
    ///     builder.add_rpath("@executable_path/lib");
    /// });
    /// ```
    pub fn for_each_slice<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Builder<'data>),
    {
        for (slice, _) in &mut self.slices {
            f(slice);
        }
    }
}

/// Get CPU type and subtype from a builder's header.
fn get_cpu_info(builder: &Builder<'_>) -> (u32, u32) {
    (builder.header.cpu_type, builder.header.cpu_subtype)
}

// Make Builder's header accessible
impl<'data> Builder<'data> {
    /// Get the CPU type from the Mach-O header.
    pub fn cpu_type(&self) -> u32 {
        self.header.cpu_type
    }

    /// Get the CPU subtype from the Mach-O header.
    pub fn cpu_subtype(&self) -> u32 {
        self.header.cpu_subtype
    }

    /// Get the file type (MH_OBJECT, MH_EXECUTE, etc.)
    pub fn file_type(&self) -> u32 {
        self.header.file_type
    }
}
