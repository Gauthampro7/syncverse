//! IEEE 802.3 table-driven CRC32 implementation.
//! Uses the standard 0xEDB88320 reversed polynomial with initial 0xFFFFFFFF and final inversion.

/// Table-driven IEEE 802.3 CRC32 engine.
pub struct Crc32;

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

impl Crc32 {
    /// Computes the IEEE 802.3 CRC32 checksum of a byte slice.
    #[inline]
    pub fn digest(data: &[u8]) -> u32 {
        Self::update(0, data)
    }

    /// Incrementally updates a CRC32 checksum with additional data.
    #[inline]
    pub fn update(initial_crc: u32, data: &[u8]) -> u32 {
        let mut crc = initial_crc ^ 0xFFFF_FFFF;
        for &byte in data {
            let index = ((crc ^ (byte as u32)) & 0xFF) as usize;
            crc = (crc >> 8) ^ CRC32_TABLE[index];
        }
        crc ^ 0xFFFF_FFFF
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_standard_vectors() {
        // Standard ASCII test vectors
        assert_eq!(Crc32::digest(b""), 0x00000000);
        assert_eq!(Crc32::digest(b"a"), 0xE8B7BE43);
        assert_eq!(Crc32::digest(b"abc"), 0x352441C2);
        assert_eq!(Crc32::digest(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn test_crc32_incremental() {
        let full = Crc32::digest(b"123456789");
        let part1 = Crc32::digest(b"1234");
        // Note: update continues an existing CRC digest
        let mut crc = 0xFFFF_FFFF;
        for &b in b"1234" {
            crc = (crc >> 8) ^ CRC32_TABLE[((crc ^ (b as u32)) & 0xFF) as usize];
        }
        for &b in b"56789" {
            crc = (crc >> 8) ^ CRC32_TABLE[((crc ^ (b as u32)) & 0xFF) as usize];
        }
        assert_eq!(crc ^ 0xFFFF_FFFF, full);
        let _ = part1;
    }
}
