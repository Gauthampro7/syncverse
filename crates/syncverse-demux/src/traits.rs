//! Container demuxer interface contracts and data models.

use std::io::{Read, Seek};
use crate::error::DemuxError;

/// Trait alias combining `Read` and `Seek` for stream readers.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Track media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackType {
    /// Video track.
    Video,
    /// Audio track.
    Audio,
    /// Subtitle or timed text track.
    Subtitle,
}

/// Information describing an individual track within a container.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    /// Container-assigned track identifier.
    pub track_id: u64,
    /// Track media type.
    pub track_type: TrackType,
    /// Codec identifier string (e.g., "V_VP9", "A_OPUS", "avc1").
    pub codec_id: String,
    /// Track language code (e.g., "eng", "en", "jpn"), if specified in container metadata.
    pub language: Option<String>,
    /// Track title or description, if specified in container metadata.
    pub title: Option<String>,
    /// Video width in pixels.
    pub width: Option<u32>,
    /// Video height in pixels.
    pub height: Option<u32>,
    /// Audio sampling rate in Hz.
    pub sample_rate: Option<f64>,
    /// Number of audio channels.
    pub channels: Option<u32>,
    /// Optional binary extradata (AVCDecoderConfigurationRecord, OpusHead, etc.).
    pub codec_private: Option<Vec<u8>>,
}

/// Media chapter metadata identifying structural boundaries in container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaChapter {
    /// Container-assigned chapter identifier.
    pub id: i64,
    /// Chapter title or name.
    pub title: String,
    /// Chapter start presentation time in milliseconds.
    pub start_ms: u64,
    /// Chapter end presentation time in milliseconds.
    pub end_ms: u64,
}

/// Global container information and track inventory.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerInfo {
    /// Container-level presentation duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Container base timescale (ticks per second or nanoseconds per tick).
    pub timescale: u64,
    /// Declared media tracks.
    pub tracks: Vec<TrackInfo>,
    /// Declared media chapters.
    pub chapters: Vec<MediaChapter>,
}

/// A demuxed raw media packet extracted from a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemuxPacket {
    /// ID of the track this packet belongs to.
    pub track_id: u64,
    /// Presentation timestamp in milliseconds.
    pub pts_ms: u64,
    /// Decode timestamp in milliseconds, if distinct from PTS.
    pub dts_ms: Option<u64>,
    /// Packet presentation duration in milliseconds, if known.
    pub duration_ms: Option<u64>,
    /// Whether this packet is an independent random-access keyframe.
    pub is_keyframe: bool,
    /// Raw compressed bitstream payload.
    pub data: Vec<u8>,
}

/// Trait implemented by pure-Rust container demuxers.
pub trait Demuxer: Send {
    /// Probes a byte stream to determine whether it matches this container format.
    fn probe(reader: &mut (dyn ReadSeek + Send)) -> Result<bool, DemuxError>;

    /// Initializes the demuxer, parses headers, and extracts container metadata.
    fn init(&mut self, reader: Box<dyn ReadSeek + Send>) -> Result<ContainerInfo, DemuxError>;

    /// Reads the next sequential media packet from the container.
    fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError>;

    /// Seeks to the nearest keyframe preceding or at `target_pts_ms`, returning the landed PTS.
    fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError>;
}
