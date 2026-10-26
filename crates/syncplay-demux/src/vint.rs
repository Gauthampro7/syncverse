//! RFC 8794 variable-length integer (VINT) decoder for EBML parsing.

use crate::error::VintError;

/// Decodes an RFC 8794 EBML variable-length integer (VINT) from a byte slice.
///
/// # Arguments
/// * `buf` - The input byte slice.
/// * `is_element_id` - Whether the VINT represents an EBML Element ID (retaining marker bit)
///   or an Element Data Size (masking out marker bit and detecting unknown size sentinels).
///
/// # Returns
/// A tuple `(raw_val, data_size, octet_width)`:
/// - `raw_val`: Raw decoded integer containing all octets including marker bit.
/// - `data_size`: `None` for element IDs or unknown/undefined sizes (all data bits set to 1);
///   `Some(value)` for valid element data sizes.
/// - `octet_width`: Number of octets consumed (1 to 8).
pub fn parse_vint(buf: &[u8], is_element_id: bool) -> Result<(u64, Option<u64>, usize), VintError> {
    if buf.is_empty() {
        return Err(VintError::UnexpectedEof {
            needed: 1,
            available: 0,
        });
    }

    let first_byte = buf[0];
    if first_byte == 0x00 {
        return Err(VintError::InvalidZeroPrefix);
    }

    let leading_zeros = first_byte.leading_zeros() as usize;
    let octet_width = leading_zeros + 1;
    if octet_width > 8 {
        return Err(VintError::InvalidZeroPrefix);
    }

    if buf.len() < octet_width {
        return Err(VintError::UnexpectedEof {
            needed: octet_width,
            available: buf.len(),
        });
    }

    let mut raw_val = 0u64;
    for &b in buf.iter().take(octet_width) {
        raw_val = (raw_val << 8) | (b as u64);
    }

    if is_element_id {
        // RFC 8794 Section 4.1: An Element ID whose data bits are all set to 1 is reserved.
        let marker_shift = (octet_width - 1) * 8 + (8 - octet_width);
        let data_mask = (1u64 << marker_shift) - 1;
        if (raw_val & data_mask) == data_mask {
            return Err(VintError::ReservedElementId);
        }
        Ok((raw_val, None, octet_width))
    } else {
        // Mask out the length marker bit to yield the data value
        let marker_shift = (octet_width - 1) * 8 + (8 - octet_width);
        let data_val = raw_val & !(1u64 << marker_shift);

        // Check for undefined/unknown size: all data bits set to 1
        let data_mask = (1u64 << marker_shift) - 1;
        if (raw_val & data_mask) == data_mask {
            Ok((raw_val, None, octet_width))
        } else {
            Ok((raw_val, Some(data_val), octet_width))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vint_parsing_scenarios() {
        // 0x81 -> 1-octet VINT with marker bit at position 7
        let (raw_id, data_size, width) = parse_vint(&[0x81], false).unwrap();
        assert_eq!(raw_id, 0x81);
        assert_eq!(data_size, Some(1));
        assert_eq!(width, 1);

        // 0x40 0x02 -> 2-octet VINT
        let (raw_id, data_size, width) = parse_vint(&[0x40, 0x02], false).unwrap();
        assert_eq!(raw_id, 0x4002);
        assert_eq!(data_size, Some(2));
        assert_eq!(width, 2);

        // 0x1F 0xFF 0xFF 0xFF -> 4-octet unknown size
        let (raw_id, data_size, width) = parse_vint(&[0x1F, 0xFF, 0xFF, 0xFF], false).unwrap();
        assert_eq!(raw_id, 0x1FFF_FFFF);
        assert_eq!(data_size, None);
        assert_eq!(width, 4);

        // Zero prefix octet
        assert_eq!(parse_vint(&[0x00, 0x12], false), Err(VintError::InvalidZeroPrefix));

        // Truncated buffer
        assert_eq!(
            parse_vint(&[0x01], false),
            Err(VintError::UnexpectedEof {
                needed: 8,
                available: 1
            })
        );

        // Reserved element ID
        assert_eq!(parse_vint(&[0xFF], true), Err(VintError::ReservedElementId));
        assert_eq!(parse_vint(&[0x7F, 0xFF], true), Err(VintError::ReservedElementId));
        assert_eq!(parse_vint(&[0x3F, 0xFF, 0xFF], true), Err(VintError::ReservedElementId));
        assert_eq!(parse_vint(&[0x1F, 0xFF, 0xFF, 0xFF], true), Err(VintError::ReservedElementId));
    }
}
