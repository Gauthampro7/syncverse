//! Pure-Rust H.264 / AVC bitstream parser and frame decoder.

pub mod decoder;
pub mod nal;
pub mod pps;
pub mod slice;
pub mod sps;

pub use decoder::H264Decoder;
pub use nal::{
    extract_nal_units, parse_annex_b, parse_avcc, unescape_rbsp, AnnexBScanner, NalHeader,
    NalUnitType,
};
pub use pps::Pps;
pub use slice::{SliceHeader, SliceType};
pub use sps::Sps;
