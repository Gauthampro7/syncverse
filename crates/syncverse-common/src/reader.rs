//! Zero-copy, bounds-checked binary slice readers for byte and bit streams.

use crate::error::CommonError;

/// Zero-copy, bounds-checked binary slice reader.
#[derive(Debug, Clone)]
pub struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    /// Constructs a new `ByteReader` wrapping the provided slice.
    #[inline]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Returns the number of unread bytes remaining in the stream.
    #[inline]
    pub const fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Returns the current read offset in bytes.
    #[inline]
    pub const fn offset(&self) -> usize {
        self.pos
    }

    /// Returns the complete underlying slice.
    #[inline]
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Peeks at the next byte without advancing the read position.
    pub fn peek_u8(&self) -> Result<u8, CommonError> {
        if self.pos < self.data.len() {
            Ok(self.data[self.pos])
        } else {
            Err(CommonError::UnexpectedEof {
                needed: 1,
                available: 0,
            })
        }
    }

    /// Reads a single byte and advances the position.
    pub fn read_u8(&mut self) -> Result<u8, CommonError> {
        if self.pos < self.data.len() {
            let byte = self.data[self.pos];
            self.pos += 1;
            Ok(byte)
        } else {
            Err(CommonError::UnexpectedEof {
                needed: 1,
                available: 0,
            })
        }
    }

    /// Reads a 16-bit big-endian unsigned integer.
    pub fn read_u16_be(&mut self) -> Result<u16, CommonError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a 16-bit little-endian unsigned integer.
    pub fn read_u16_le(&mut self) -> Result<u16, CommonError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a 32-bit big-endian unsigned integer.
    pub fn read_u32_be(&mut self) -> Result<u32, CommonError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a 32-bit little-endian unsigned integer.
    pub fn read_u32_le(&mut self) -> Result<u32, CommonError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a 64-bit big-endian unsigned integer.
    pub fn read_u64_be(&mut self) -> Result<u64, CommonError> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads a 64-bit little-endian unsigned integer.
    pub fn read_u64_le(&mut self) -> Result<u64, CommonError> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads a 16-bit big-endian signed integer.
    pub fn read_i16_be(&mut self) -> Result<i16, CommonError> {
        let bytes = self.read_bytes(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Reads an exact number of bytes as a subslice.
    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], CommonError> {
        if self.remaining() < len {
            return Err(CommonError::UnexpectedEof {
                needed: len,
                available: self.remaining(),
            });
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    /// Skips a given number of bytes forward.
    pub fn skip(&mut self, len: usize) -> Result<(), CommonError> {
        if self.remaining() < len {
            return Err(CommonError::UnexpectedEof {
                needed: len,
                available: self.remaining(),
            });
        }
        self.pos += len;
        Ok(())
    }

    /// Seeks to an absolute offset in the slice.
    pub fn seek(&mut self, pos: usize) -> Result<(), CommonError> {
        if pos > self.data.len() {
            return Err(CommonError::OutOfBounds {
                requested: pos,
                max: self.data.len(),
            });
        }
        self.pos = pos;
        Ok(())
    }
}

/// Bit-level stream reader for parsing bitstream fields.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 0..=7 (0 is most-significant bit of current byte)
}

impl<'a> BitReader<'a> {
    /// Constructs a new `BitReader`.
    pub const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Returns the number of unread bits remaining.
    pub fn remaining_bits(&self) -> usize {
        let total_bits = self.data.len() * 8;
        let consumed_bits = self.byte_pos * 8 + (self.bit_pos as usize);
        total_bits.saturating_sub(consumed_bits)
    }

    /// Reads a single bit from the bitstream.
    pub fn read_bit(&mut self) -> Result<bool, CommonError> {
        if self.byte_pos >= self.data.len() {
            return Err(CommonError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }
        let byte = self.data[self.byte_pos];
        let shift = 7 - self.bit_pos;
        let bit = ((byte >> shift) & 1) != 0;

        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(bit)
    }

    /// Reads up to 64 bits from the bitstream.
    pub fn read_bits(&mut self, count: usize) -> Result<u64, CommonError> {
        if count > 64 {
            return Err(CommonError::InvalidData("cannot read more than 64 bits at once".into()));
        }
        if self.remaining_bits() < count {
            return Err(CommonError::UnexpectedEof {
                needed: count,
                available: self.remaining_bits(),
            });
        }

        let mut result = 0u64;
        for _ in 0..count {
            let bit = self.read_bit()?;
            result = (result << 1) | (bit as u64);
        }
        Ok(result)
    }

    /// Aligns the bitstream reader to the next byte boundary by discarding any remaining bits in the current byte.
    pub fn byte_align(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    /// Reads an unsigned Exponential-Golomb coded integer `ue(v)` per ITU-T H.264 Section 9.1.
    pub fn read_ue(&mut self) -> Result<u32, CommonError> {
        let mut leading_zeros = 0usize;
        while !self.read_bit()? {
            leading_zeros += 1;
            if leading_zeros > 31 {
                return Err(CommonError::InvalidData(
                    "Exp-Golomb leading zeros exceed 31 bits".into(),
                ));
            }
        }
        if leading_zeros == 0 {
            return Ok(0);
        }
        let info = self.read_bits(leading_zeros)?;
        let code_num = ((1u64 << leading_zeros) - 1) + info;
        if code_num > u32::MAX as u64 {
            return Err(CommonError::InvalidData(
                "Exp-Golomb code_num exceeds u32::MAX".into(),
            ));
        }
        Ok(code_num as u32)
    }

    /// Reads a signed Exponential-Golomb coded integer `se(v)` per ITU-T H.264 Section 9.1.
    pub fn read_se(&mut self) -> Result<i32, CommonError> {
        let k = self.read_ue()? as u64;
        if k == 0 {
            return Ok(0);
        }
        if (k & 1) != 0 {
            let val = k.div_ceil(2) as i64;
            if val > i32::MAX as i64 {
                return Err(CommonError::InvalidData("Exp-Golomb se(v) overflow".into()));
            }
            Ok(val as i32)
        } else {
            let val = -((k / 2) as i64);
            if val < i32::MIN as i64 {
                return Err(CommonError::InvalidData("Exp-Golomb se(v) underflow".into()));
            }
            Ok(val as i32)
        }
    }

    /// Reads a truncated Exponential-Golomb coded integer `te(v)` given an upper bound `max`.
    pub fn read_te(&mut self, max: u32) -> Result<u32, CommonError> {
        if max <= 1 {
            if max == 1 {
                let bit = self.read_bit()?;
                Ok(if bit { 0 } else { 1 })
            } else {
                Ok(0)
            }
        } else {
            self.read_ue()
        }
    }

    /// Returns `true` if there are more RBSP data bits available before `rbsp_trailing_bits`.
    ///
    /// Per ITU-T H.264 Section 7.2:
    /// Returns false if end of stream is reached, or if the next bit is 1 and all subsequent bits
    /// in the bitstream are 0.
    pub fn has_more_rbsp_data(&self) -> bool {
        if self.remaining_bits() == 0 {
            return false;
        }
        let mut clone = self.clone();
        match clone.read_bit() {
            Ok(false) => true,
            Ok(true) => {
                while clone.remaining_bits() > 0 {
                    if let Ok(true) = clone.read_bit() {
                        return true;
                    }
                }
                false
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_reader_primitives() {
        let buf = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        let mut reader = ByteReader::new(&buf);

        assert_eq!(reader.remaining(), 8);
        assert_eq!(reader.read_u16_be().unwrap(), 0x1234);
        assert_eq!(reader.read_u32_be().unwrap(), 0x56789ABC);
        assert_eq!(reader.read_u16_le().unwrap(), 0xF0DE);
        assert_eq!(reader.remaining(), 0);
        assert!(reader.read_u8().is_err());
    }

    #[test]
    fn test_bit_reader() {
        let buf = [0b1011_0011, 0b1111_0000];
        let mut reader = BitReader::new(&buf);

        assert!(reader.read_bit().unwrap());
        assert!(!reader.read_bit().unwrap());
        assert_eq!(reader.read_bits(4).unwrap(), 0b1100);
        assert_eq!(reader.read_bits(2).unwrap(), 0b11);
        assert_eq!(reader.read_bits(4).unwrap(), 0b1111);
        assert_eq!(reader.read_bits(4).unwrap(), 0b0000);
        assert_eq!(reader.remaining_bits(), 0);
    }

    #[test]
    fn test_exp_golomb_ue_and_se() {
        // Bitstream encoding ue(v):
        // 0 -> '1'
        // 1 -> '010'
        // 2 -> '011'
        // 3 -> '00100'
        // 4 -> '00101'
        // 5 -> '00110'
        // 6 -> '00111'
        // Pack: 1 010 011 0 0100 0010 1 00110 00 111 (plus padding)
        let bits = [
            0b1010_0110,
            0b0100_0010,
            0b1001_1000,
            0b1110_0000,
        ];
        let mut reader = BitReader::new(&bits);
        assert_eq!(reader.read_ue().unwrap(), 0);
        assert_eq!(reader.read_ue().unwrap(), 1);
        assert_eq!(reader.read_ue().unwrap(), 2);
        assert_eq!(reader.read_ue().unwrap(), 3);
        assert_eq!(reader.read_ue().unwrap(), 4);
        assert_eq!(reader.read_ue().unwrap(), 5);
        assert_eq!(reader.read_ue().unwrap(), 6);

        // Test se(v):
        // code_num 0 -> 0
        // code_num 1 -> 1
        // code_num 2 -> -1
        // code_num 3 -> 2
        // code_num 4 -> -2
        // code_num 5 -> 3
        // code_num 6 -> -3
        let mut reader2 = BitReader::new(&bits);
        assert_eq!(reader2.read_se().unwrap(), 0);
        assert_eq!(reader2.read_se().unwrap(), 1);
        assert_eq!(reader2.read_se().unwrap(), -1);
        assert_eq!(reader2.read_se().unwrap(), 2);
        assert_eq!(reader2.read_se().unwrap(), -2);
        assert_eq!(reader2.read_se().unwrap(), 3);
        assert_eq!(reader2.read_se().unwrap(), -3);
    }

    #[test]
    fn test_exp_golomb_te_and_alignment() {
        // te(v) with max = 1: bit '1' -> 0, bit '0' -> 1
        let buf = [0b1000_0000];
        let mut reader = BitReader::new(&buf);
        assert_eq!(reader.read_te(1).unwrap(), 0);
        assert_eq!(reader.read_te(1).unwrap(), 1);

        reader.byte_align();
        assert_eq!(reader.remaining_bits(), 0);
    }

    #[test]
    fn test_has_more_rbsp_data() {
        // RBSP with data followed by trailing bits: stop bit '1' then zero or more '0'
        let buf = [0b1010_1100]; // 5 data bits '10101', stop bit '1', 2 padding zeros '00'
        let mut reader = BitReader::new(&buf);
        assert!(reader.has_more_rbsp_data());
        assert_eq!(reader.read_bits(5).unwrap(), 0b10101);
        assert!(!reader.has_more_rbsp_data());
    }
}
