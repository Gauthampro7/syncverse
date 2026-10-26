//! Slash-command lexer, grammar parser, and media control triggers.

use crate::error::ChatError;

/// Parsed slash-command representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedSlashCommand {
    /// `/seek <time>`: Requests seek synchronization to target presentation timestamp in milliseconds.
    Seek {
        /// Absolute target timestamp in milliseconds.
        target_pts_ms: u64,
    },
    /// `/bookmark [label]`: Marks a timeline bookmark at current presentation timestamp.
    Bookmark {
        /// Bookmark label.
        label: Option<String>,
        /// Presentation timestamp in milliseconds.
        pts_ms: u64,
    },
    /// `/clip <start> <end> [label]`: Specifies a media interval for clipping.
    Clip {
        /// Start presentation timestamp in milliseconds.
        start_pts_ms: u64,
        /// End presentation timestamp in milliseconds.
        end_pts_ms: u64,
        /// Clip label.
        label: Option<String>,
    },
    /// `/poll "Question" "Option 1" "Option 2" ...`: Initiates a room voting poll.
    Poll {
        /// Question text.
        question: String,
        /// List of poll choice options.
        options: Vec<String>,
    },
    /// `/sync-status`: Requests a local synchronization diagnostics report.
    SyncStatus,
}

/// Type alias for `ParsedSlashCommand` conforming to the architectural contract.
pub type ParsedCommand = ParsedSlashCommand;

/// Parses a time string (HH:MM:SS.mmm, MM:SS, seconds, or relative offset +15s, -10s).
pub fn parse_time_to_ms(time_str: &str, current_pts_ms: Option<u64>) -> Result<u64, ChatError> {
    let s = time_str.trim();

    // Check relative offset: e.g. +15s, -10s
    if s.starts_with('+') || s.starts_with('-') {
        let is_neg = s.starts_with('-');
        let rest = &s[1..];
        if rest.starts_with('+') || rest.starts_with('-') {
            return Err(ChatError::InvalidCommand(format!("invalid relative time offset: {}", s)));
        }
        let num_str = rest.trim_end_matches(['s', 'S']);
        let secs: f64 = num_str.parse().map_err(|_| {
            ChatError::InvalidCommand(format!("invalid relative time offset: {}", s))
        })?;
        if !secs.is_finite() || secs < 0.0 || secs > 315_360_000.0 {
            return Err(ChatError::InvalidCommand(format!("relative time offset out of bounds: {}", s)));
        }
        let delta_ms = (secs * 1000.0) as u64;
        let base = current_pts_ms.unwrap_or(0);
        if is_neg {
            return Ok(base.saturating_sub(delta_ms));
        } else {
            return Ok(base.saturating_add(delta_ms));
        }
    }

    // Check HH:MM:SS or MM:SS
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.len() {
            2 => {
                // MM:SS or MM:SS.mmm
                let mins: u64 = parts[0].parse().map_err(|_| {
                    ChatError::InvalidCommand(format!("invalid minutes in time: {}", s))
                })?;
                let secs: f64 = parts[1].parse().map_err(|_| {
                    ChatError::InvalidCommand(format!("invalid seconds in time: {}", s))
                })?;
                if !secs.is_finite() || secs < 0.0 || secs >= 60.0 {
                    return Err(ChatError::InvalidCommand(format!("invalid seconds in time: {}", s)));
                }
                let total_ms = mins * 60 * 1000 + (secs * 1000.0) as u64;
                return Ok(total_ms);
            }
            3 => {
                // HH:MM:SS or HH:MM:SS.mmm
                let hours: u64 = parts[0].parse().map_err(|_| {
                    ChatError::InvalidCommand(format!("invalid hours in time: {}", s))
                })?;
                let mins: u64 = parts[1].parse().map_err(|_| {
                    ChatError::InvalidCommand(format!("invalid minutes in time: {}", s))
                })?;
                if mins >= 60 {
                    return Err(ChatError::InvalidCommand(format!("minutes must be < 60 in time: {}", s)));
                }
                let secs: f64 = parts[2].parse().map_err(|_| {
                    ChatError::InvalidCommand(format!("invalid seconds in time: {}", s))
                })?;
                if !secs.is_finite() || secs < 0.0 || secs >= 60.0 {
                    return Err(ChatError::InvalidCommand(format!("invalid seconds in time: {}", s)));
                }
                let total_ms = hours * 3600 * 1000 + mins * 60 * 1000 + (secs * 1000.0) as u64;
                return Ok(total_ms);
            }
            _ => return Err(ChatError::InvalidCommand(format!("malformed time string: {}", s))),
        }
    }

    // Raw seconds (e.g. "123.4")
    let raw_secs: f64 = s.trim_end_matches(['s', 'S']).parse().map_err(|_| {
        ChatError::InvalidCommand(format!("invalid numeric time string: {}", s))
    })?;
    if !raw_secs.is_finite() || raw_secs < 0.0 || raw_secs > 315_360_000.0 {
        return Err(ChatError::InvalidCommand(format!("time value out of reasonable range or non-finite: {}", s)));
    }
    Ok((raw_secs * 1000.0) as u64)
}

/// Slash command parser.
pub struct SlashCommandParser;

impl SlashCommandParser {
    /// Parses a chat slash command string into a `ParsedSlashCommand`.
    pub fn parse(input: &str, current_pts_ms: Option<u64>) -> Result<ParsedSlashCommand, ChatError> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return Err(ChatError::InvalidCommand("command must begin with /".into()));
        }

        let mut parts = trimmed[1..].split_whitespace();
        let cmd = parts
            .next()
            .ok_or_else(|| ChatError::InvalidCommand("empty slash command".into()))?;

        match cmd.to_lowercase().as_str() {
            "seek" => {
                let time_arg = parts
                    .next()
                    .ok_or_else(|| ChatError::InvalidCommand("/seek requires a time parameter".into()))?;
                let target_pts_ms = parse_time_to_ms(time_arg, current_pts_ms)?;
                Ok(ParsedSlashCommand::Seek { target_pts_ms })
            }
            "bookmark" => {
                let rest: Vec<&str> = parts.collect();
                let label = if rest.is_empty() {
                    None
                } else {
                    Some(rest.join(" ").trim_matches('"').to_string())
                };
                let pts_ms = current_pts_ms.unwrap_or(0);
                Ok(ParsedSlashCommand::Bookmark { label, pts_ms })
            }
            "clip" => {
                let start_arg = parts
                    .next()
                    .ok_or_else(|| ChatError::InvalidCommand("/clip requires start and end times".into()))?;
                let end_arg = parts
                    .next()
                    .ok_or_else(|| ChatError::InvalidCommand("/clip requires an end time".into()))?;
                let rest: Vec<&str> = parts.collect();
                let label = if rest.is_empty() {
                    None
                } else {
                    Some(rest.join(" ").trim_matches('"').to_string())
                };
                let start_pts_ms = parse_time_to_ms(start_arg, current_pts_ms)?;
                let end_pts_ms = parse_time_to_ms(end_arg, current_pts_ms)?;
                if start_pts_ms > end_pts_ms {
                    return Err(ChatError::InvalidCommand(format!(
                        "/clip start time ({} ms) cannot be greater than end time ({} ms)",
                        start_pts_ms, end_pts_ms
                    )));
                }
                Ok(ParsedSlashCommand::Clip {
                    start_pts_ms,
                    end_pts_ms,
                    label,
                })
            }
            "poll" => {
                let remaining = trimmed[trimmed.find("poll").unwrap() + 4..].trim();
                if remaining.contains('|') {
                    // Pipe-delimited: /poll <question> | <opt1> | <opt2> ...
                    let pipe_parts: Vec<&str> = remaining
                        .split('|')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if pipe_parts.len() < 2 {
                        return Err(ChatError::InvalidCommand(
                            "/poll with pipes requires a question and at least one option: /poll <question> | <opt1> | <opt2>".into(),
                        ));
                    }
                    let question = pipe_parts[0].trim_matches('"').to_string();
                    let options: Vec<String> = pipe_parts[1..]
                        .iter()
                        .map(|s| s.trim_matches('"').to_string())
                        .collect();
                    Ok(ParsedSlashCommand::Poll { question, options })
                } else {
                    // Quote-delimited: /poll "Question" "Opt1" "Opt2" ...
                    let mut quoted = Vec::new();
                    let mut in_quotes = false;
                    let mut current = String::new();

                    for c in remaining.chars() {
                        if c == '"' {
                            if in_quotes {
                                quoted.push(current.clone());
                                current.clear();
                                in_quotes = false;
                            } else {
                                in_quotes = true;
                            }
                        } else if in_quotes {
                            current.push(c);
                        }
                    }

                    if quoted.len() < 2 {
                        return Err(ChatError::InvalidCommand(
                            "/poll requires a question and at least one option (use quotes: /poll \"Question\" \"Opt1\" or pipes: /poll Question | Opt1 | Opt2)".into(),
                        ));
                    }

                    let question = quoted.remove(0);
                    Ok(ParsedSlashCommand::Poll {
                        question,
                        options: quoted,
                    })
                }
            }
            "sync-status" => Ok(ParsedSlashCommand::SyncStatus),
            other => Err(ChatError::InvalidCommand(format!(
                "unrecognized slash command: /{}",
                other
            ))),
        }
    }
}

/// Media timestamp bookmark created via `/bookmark`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    /// Optional descriptive label for the bookmark.
    pub label: Option<String>,
    /// Presentation timestamp in milliseconds.
    pub pts_ms: u64,
    /// Wall-clock creation timestamp in milliseconds.
    pub created_at_ms: u64,
}

/// Media clip definition created via `/clip`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipDefinition {
    /// Start presentation timestamp in milliseconds.
    pub start_pts_ms: u64,
    /// End presentation timestamp in milliseconds.
    pub end_pts_ms: u64,
    /// Optional clip title or description.
    pub title: Option<String>,
}

/// Active voting poll created via `/poll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePoll {
    /// Unique identifier for this poll.
    pub poll_id: u32,
    /// Poll question.
    pub question: String,
    /// List of choice options.
    pub options: Vec<String>,
    /// Vote counts per option.
    pub votes: Vec<u32>,
    /// Wall-clock creation timestamp in milliseconds.
    pub created_at_ms: u64,
}

/// Synchronization diagnostics used for formatting `/sync-status` reports.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncDiagnostics {
    /// Round-trip time in milliseconds.
    pub rtt_ms: f64,
    /// Clock offset in milliseconds.
    pub clock_offset_ms: f64,
    /// PLL playback speed multiplier (e.g. 1.000).
    pub speed_multiplier: f64,
    /// Number of connected peers.
    pub peer_count: usize,
    /// Current media presentation timestamp in milliseconds.
    pub current_pts_ms: u64,
    /// Whether playback is currently paused.
    pub is_paused: bool,
    /// Current PLL zone name (e.g. "Deadband", "MicroSpeed", "HardSeek").
    pub pll_zone: String,
}

impl Default for SyncDiagnostics {
    fn default() -> Self {
        Self {
            rtt_ms: 0.0,
            clock_offset_ms: 0.0,
            speed_multiplier: 1.0,
            peer_count: 0,
            current_pts_ms: 0,
            is_paused: false,
            pll_zone: "Deadband".to_string(),
        }
    }
}

/// Formats a diagnostics snapshot into a human-readable synchronization status report.
pub fn format_sync_status_report(diag: &SyncDiagnostics) -> String {
    format!(
        "=== Syncplay-Alt Sync Status ===\n\
         • Playback PTS:   {:.3}s ({} ms)\n\
         • State:          {}\n\
         • Clock Offset θ: {:+.2} ms\n\
         • Round-Trip RTT: {:.2} ms\n\
         • Speed Steering: {:.4}x\n\
         • PLL Zone:       {}\n\
         • Connected Peers: {}\n\
         ================================",
        diag.current_pts_ms as f64 / 1000.0,
        diag.current_pts_ms,
        if diag.is_paused { "PAUSED" } else { "PLAYING" },
        diag.clock_offset_ms,
        diag.rtt_ms,
        diag.speed_multiplier,
        diag.pll_zone,
        diag.peer_count,
    )
}

/// Resulting action produced by the `SlashCommandDispatcher`.
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchAction {
    /// Seek event to target PTS in milliseconds.
    Seek {
        /// Target presentation timestamp in milliseconds.
        target_pts_ms: u64,
    },
    /// Created media timestamp bookmark.
    Bookmark(Bookmark),
    /// Created clip definition with start/end media timestamps.
    Clip(ClipDefinition),
    /// Created active poll.
    Poll(ActivePoll),
    /// Formatted synchronization diagnostic report string.
    SyncStatus {
        /// Formatted diagnostic report text.
        report: String,
        /// Underlying diagnostics snapshot.
        diagnostics: SyncDiagnostics,
    },
}

/// Dispatcher that parses slash-command strings and transforms them into executable room/sync actions.
#[derive(Debug, Default)]
pub struct SlashCommandDispatcher {
    bookmarks: Vec<Bookmark>,
    clips: Vec<ClipDefinition>,
    active_poll: Option<ActivePoll>,
    next_poll_id: u32,
    sync_diagnostics: Option<SyncDiagnostics>,
}

impl SlashCommandDispatcher {
    /// Creates a new dispatcher.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets or updates the current sync diagnostics provider.
    pub fn set_sync_diagnostics(&mut self, diag: SyncDiagnostics) {
        self.sync_diagnostics = Some(diag);
    }

    /// Dispatches a raw command input string at the current presentation time.
    pub fn dispatch(
        &mut self,
        input: &str,
        current_pts_ms: Option<u64>,
    ) -> Result<DispatchAction, ChatError> {
        let parsed = SlashCommandParser::parse(input, current_pts_ms)?;
        self.dispatch_parsed(&parsed, current_pts_ms)
    }

    /// Dispatches an already parsed command.
    pub fn dispatch_parsed(
        &mut self,
        command: &ParsedSlashCommand,
        current_pts_ms: Option<u64>,
    ) -> Result<DispatchAction, ChatError> {
        match command {
            ParsedSlashCommand::Seek { target_pts_ms } => {
                Ok(DispatchAction::Seek { target_pts_ms: *target_pts_ms })
            }
            ParsedSlashCommand::Bookmark { label, pts_ms } => {
                let bm = Bookmark {
                    label: label.clone(),
                    pts_ms: *pts_ms,
                    created_at_ms: current_pts_ms.unwrap_or(0),
                };
                self.bookmarks.push(bm.clone());
                Ok(DispatchAction::Bookmark(bm))
            }
            ParsedSlashCommand::Clip { start_pts_ms, end_pts_ms, label } => {
                let clip = ClipDefinition {
                    start_pts_ms: *start_pts_ms,
                    end_pts_ms: *end_pts_ms,
                    title: label.clone(),
                };
                self.clips.push(clip.clone());
                Ok(DispatchAction::Clip(clip))
            }
            ParsedSlashCommand::Poll { question, options } => {
                self.next_poll_id += 1;
                let num_opts = options.len();
                let poll = ActivePoll {
                    poll_id: self.next_poll_id,
                    question: question.clone(),
                    options: options.clone(),
                    votes: vec![0; num_opts],
                    created_at_ms: current_pts_ms.unwrap_or(0),
                };
                self.active_poll = Some(poll.clone());
                Ok(DispatchAction::Poll(poll))
            }
            ParsedSlashCommand::SyncStatus => {
                let diag = self.sync_diagnostics.clone().unwrap_or_else(|| {
                    let mut d = SyncDiagnostics::default();
                    if let Some(pts) = current_pts_ms {
                        d.current_pts_ms = pts;
                    }
                    d
                });
                let report = format_sync_status_report(&diag);
                Ok(DispatchAction::SyncStatus {
                    report,
                    diagnostics: diag,
                })
            }
        }
    }

    /// Returns all saved bookmarks.
    pub fn bookmarks(&self) -> &[Bookmark] {
        &self.bookmarks
    }

    /// Returns all defined clips.
    pub fn clips(&self) -> &[ClipDefinition] {
        &self.clips
    }

    /// Returns the currently active poll, if any.
    pub fn active_poll(&self) -> Option<&ActivePoll> {
        self.active_poll.as_ref()
    }

    /// Casts a vote in the active poll for the given 0-based option index.
    pub fn vote_poll(&mut self, option_index: usize) -> Result<(), ChatError> {
        let poll = self.active_poll.as_mut().ok_or_else(|| {
            ChatError::InvalidCommand("no active poll to vote on".into())
        })?;
        if option_index >= poll.votes.len() {
            return Err(ChatError::InvalidCommand(format!(
                "invalid option index {}; poll only has {} options",
                option_index, poll.votes.len()
            )));
        }
        poll.votes[option_index] += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_seek_commands() {
        // /seek 01:23:45.500 -> 5025500 ms
        let cmd1 = SlashCommandParser::parse("/seek 01:23:45.500", None).unwrap();
        assert_eq!(cmd1, ParsedSlashCommand::Seek { target_pts_ms: 5025500 });

        // /seek 14:30 -> 870000 ms
        let cmd2 = SlashCommandParser::parse("/seek 14:30", None).unwrap();
        assert_eq!(cmd2, ParsedSlashCommand::Seek { target_pts_ms: 870000 });

        // /seek +15s at current position 100,000 ms -> 115000 ms
        let cmd3 = SlashCommandParser::parse("/seek +15s", Some(100000)).unwrap();
        assert_eq!(cmd3, ParsedSlashCommand::Seek { target_pts_ms: 115000 });

        // /seek -10s at current position 100,000 ms -> 90000 ms
        let cmd4 = SlashCommandParser::parse("/seek -10s", Some(100000)).unwrap();
        assert_eq!(cmd4, ParsedSlashCommand::Seek { target_pts_ms: 90000 });
    }

    #[test]
    fn test_parse_poll_and_bookmark() {
        let cmd_poll = SlashCommandParser::parse(r#"/poll "Best anime?" "Steins;Gate" "FMA""#, None).unwrap();
        match cmd_poll {
            ParsedSlashCommand::Poll { question, options } => {
                assert_eq!(question, "Best anime?");
                assert_eq!(options, vec!["Steins;Gate", "FMA"]);
            }
            _ => panic!("Expected Poll"),
        }

        let cmd_bm = SlashCommandParser::parse("/bookmark epic_moment", Some(45000)).unwrap();
        assert_eq!(
            cmd_bm,
            ParsedSlashCommand::Bookmark {
                label: Some("epic_moment".into()),
                pts_ms: 45000
            }
        );

        let cmd_sync = SlashCommandParser::parse("/sync-status", None).unwrap();
        assert_eq!(cmd_sync, ParsedSlashCommand::SyncStatus);
    }

    #[test]
    fn test_parse_poll_pipes() {
        let cmd = SlashCommandParser::parse("/poll Best Framework? | Actix | Axum | Rocket", None).unwrap();
        match cmd {
            ParsedSlashCommand::Poll { question, options } => {
                assert_eq!(question, "Best Framework?");
                assert_eq!(options, vec!["Actix", "Axum", "Rocket"]);
            }
            _ => panic!("Expected Poll"),
        }
    }

    #[test]
    fn test_parse_clip_and_validation() {
        let cmd = SlashCommandParser::parse("/clip 01:00 02:30 Solo Riff", None).unwrap();
        assert_eq!(
            cmd,
            ParsedSlashCommand::Clip {
                start_pts_ms: 60000,
                end_pts_ms: 150000,
                label: Some("Solo Riff".into()),
            }
        );

        // Validation: start > end must error
        let err = SlashCommandParser::parse("/clip 02:00 01:00 Inverted", None);
        assert!(err.is_err());
    }

    #[test]
    fn test_slash_command_dispatcher() {
        let mut dispatcher = SlashCommandDispatcher::new();

        // 1. Dispatch seek
        let action_seek = dispatcher.dispatch("/seek 10:00", None).unwrap();
        assert_eq!(action_seek, DispatchAction::Seek { target_pts_ms: 600000 });

        // 2. Dispatch bookmark
        let action_bm = dispatcher.dispatch("/bookmark Key Scene", Some(12500)).unwrap();
        match action_bm {
            DispatchAction::Bookmark(bm) => {
                assert_eq!(bm.label.as_deref(), Some("Key Scene"));
                assert_eq!(bm.pts_ms, 12500);
            }
            _ => panic!("Expected Bookmark"),
        }
        assert_eq!(dispatcher.bookmarks().len(), 1);

        // 3. Dispatch clip
        let action_clip = dispatcher.dispatch("/clip 10.5s 20s Guitar Solo", None).unwrap();
        match action_clip {
            DispatchAction::Clip(clip) => {
                assert_eq!(clip.start_pts_ms, 10500);
                assert_eq!(clip.end_pts_ms, 20000);
                assert_eq!(clip.title.as_deref(), Some("Guitar Solo"));
            }
            _ => panic!("Expected Clip"),
        }
        assert_eq!(dispatcher.clips().len(), 1);

        // 4. Dispatch poll with pipes & vote
        let action_poll = dispatcher.dispatch("/poll Coffee or Tea? | Coffee | Tea", None).unwrap();
        match action_poll {
            DispatchAction::Poll(poll) => {
                assert_eq!(poll.question, "Coffee or Tea?");
                assert_eq!(poll.options.len(), 2);
                assert_eq!(poll.votes, vec![0, 0]);
            }
            _ => panic!("Expected Poll"),
        }

        dispatcher.vote_poll(0).unwrap();
        dispatcher.vote_poll(0).unwrap();
        dispatcher.vote_poll(1).unwrap();
        assert_eq!(dispatcher.active_poll().unwrap().votes, vec![2, 1]);
        assert!(dispatcher.vote_poll(99).is_err());

        // 5. Dispatch sync-status
        let diag = SyncDiagnostics {
            rtt_ms: 320.5,
            clock_offset_ms: -4.2,
            speed_multiplier: 1.0025,
            peer_count: 3,
            current_pts_ms: 45000,
            is_paused: false,
            pll_zone: "MicroSpeed".to_string(),
        };
        dispatcher.set_sync_diagnostics(diag);
        let action_sync = dispatcher.dispatch("/sync-status", Some(45000)).unwrap();
        match action_sync {
            DispatchAction::SyncStatus { report, diagnostics } => {
                assert!(report.contains("320.50 ms"));
                assert!(report.contains("MicroSpeed"));
                assert_eq!(diagnostics.peer_count, 3);
            }
            _ => panic!("Expected SyncStatus"),
        }
    }
}
