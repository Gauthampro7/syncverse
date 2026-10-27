//! Container demuxing subsystem for Matroska (MKV/EBML) and ISOBMFF (MP4) containers.

#![deny(warnings)]
#![deny(missing_docs)]

pub mod ebml;
pub mod error;
pub mod isobmff;
pub mod subtitles;
pub mod synthetic;
pub mod traits;
pub mod universal;
pub mod vint;

pub use ebml::{
    compute_simpleblock_pts, unpack_lacing, CueEntry, EbmlDemuxer, SYNTHETIC_EBML_VECTOR,
};
pub use error::{DemuxError, VintError};
pub use isobmff::{
    BoxHeader, FourCc, IsoBmffDemuxer, Mp4TrackState, SampleTableResolver, StscEntry,
    SYNTHETIC_MP4_VECTOR,
};
pub use subtitles::{
    clean_ass_text, clean_srt_text, clean_vtt_text, detect_subtitle_language, parse_ass, parse_srt,
    parse_subtitle_file, parse_vtt, scan_external_subtitles, SubtitleParseError,
};
pub use synthetic::{
    build_synthetic_full_mkv, build_synthetic_full_mp4, get_synthetic_full_mkv,
    get_synthetic_full_mp4,
};
pub use traits::{ContainerInfo, DemuxPacket, Demuxer, ReadSeek, TrackInfo, TrackType};
pub use universal::UniversalDemuxer;
pub use vint::parse_vint;


