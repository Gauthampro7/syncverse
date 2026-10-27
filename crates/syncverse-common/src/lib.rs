//! Common foundational types, readers, CRC32, and error definitions for Syncplay-Alt.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod crc;
pub mod error;
pub mod path;
pub mod reader;
pub mod time;

pub use crc::Crc32;
pub use error::CommonError;
pub use path::normalize_target_path;
pub use reader::{BitReader, ByteReader};
pub use time::{MediaTime, WallTime};
