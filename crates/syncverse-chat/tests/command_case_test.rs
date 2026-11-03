//! Integration tests for case-insensitive slash command parsing in `syncverse-chat`.
//!
//! Verifies that commands such as `/POLL`, `/Poll`, `/SEEK`, `/Seek`, `/BOOKMARK`,
//! `/CLIP`, and `/SYNC-STATUS` parse reliably regardless of casing without panicking.

#![deny(warnings)]

use syncverse_chat::{ChatError, ParsedSlashCommand, SlashCommandParser};

#[test]
fn test_case_insensitive_poll_quoted_syntax() {
    let cmd1 = SlashCommandParser::parse(
        r#"/POLL "Favorite Movie?" "Interstellar" "Blade Runner" "Arrival""#,
        None,
    )
    .expect("Uppercase /POLL should parse successfully");

    match cmd1 {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Favorite Movie?");
            assert_eq!(options, vec!["Interstellar", "Blade Runner", "Arrival"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    let cmd2 = SlashCommandParser::parse(
        r#"/Poll "Take a break?" "Yes" "No""#,
        Some(42_000),
    )
    .expect("Title-cased /Poll should parse successfully");

    match cmd2 {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Take a break?");
            assert_eq!(options, vec!["Yes", "No"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    let cmd3 = SlashCommandParser::parse(
        r#"/pOlL "Mixed Case?" "True" "False""#,
        None,
    )
    .expect("Mixed-cased /pOlL should parse successfully");

    match cmd3 {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Mixed Case?");
            assert_eq!(options, vec!["True", "False"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }
}

#[test]
fn test_case_insensitive_poll_pipe_syntax() {
    let cmd = SlashCommandParser::parse(
        "/POLL Best Snack | Popcorn | Pretzels | Nachos",
        None,
    )
    .expect("Uppercase /POLL with pipe syntax should parse successfully");

    match cmd {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Best Snack");
            assert_eq!(options, vec!["Popcorn", "Pretzels", "Nachos"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    let cmd_mixed = SlashCommandParser::parse(
        "/Poll Volume check | Good | Too loud | Too quiet",
        None,
    )
    .expect("Title-cased /Poll with pipe syntax should parse successfully");

    match cmd_mixed {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Volume check");
            assert_eq!(options, vec!["Good", "Too loud", "Too quiet"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }
}

#[test]
fn test_case_insensitive_seek() {
    // Uppercase /SEEK with timecode
    let cmd1 = SlashCommandParser::parse("/SEEK 01:23.450", None)
        .expect("Uppercase /SEEK should parse");
    match cmd1 {
        ParsedSlashCommand::Seek { target_pts_ms } => {
            assert_eq!(target_pts_ms, 83_450);
        }
        _ => panic!("Expected ParsedSlashCommand::Seek"),
    }

    // Titlecase /Seek with relative offset
    let cmd2 = SlashCommandParser::parse("/Seek +15s", Some(100_000))
        .expect("Titlecase /Seek should parse");
    match cmd2 {
        ParsedSlashCommand::Seek { target_pts_ms } => {
            assert_eq!(target_pts_ms, 115_000);
        }
        _ => panic!("Expected ParsedSlashCommand::Seek"),
    }

    // Mixed case /sEeK with negative relative offset
    let cmd3 = SlashCommandParser::parse("/sEeK -20s", Some(50_000))
        .expect("Mixed case /sEeK should parse");
    match cmd3 {
        ParsedSlashCommand::Seek { target_pts_ms } => {
            assert_eq!(target_pts_ms, 30_000);
        }
        _ => panic!("Expected ParsedSlashCommand::Seek"),
    }

    // Uppercase with raw seconds
    let cmd4 = SlashCommandParser::parse("/SEEK 90", None)
        .expect("Uppercase /SEEK with raw seconds should parse");
    match cmd4 {
        ParsedSlashCommand::Seek { target_pts_ms } => {
            assert_eq!(target_pts_ms, 90_000);
        }
        _ => panic!("Expected ParsedSlashCommand::Seek"),
    }
}

#[test]
fn test_case_insensitive_bookmark() {
    let current_pts = 123_456;

    // Uppercase with quoted label
    let cmd1 = SlashCommandParser::parse("/BOOKMARK \"Opening credits\"", Some(current_pts))
        .expect("Uppercase /BOOKMARK should parse");
    match cmd1 {
        ParsedSlashCommand::Bookmark { label, pts_ms } => {
            assert_eq!(label.as_deref(), Some("Opening credits"));
            assert_eq!(pts_ms, current_pts);
        }
        _ => panic!("Expected ParsedSlashCommand::Bookmark"),
    }

    // Titlecase with unquoted label
    let cmd2 = SlashCommandParser::parse("/Bookmark Climax scene", Some(current_pts))
        .expect("Titlecase /Bookmark should parse");
    match cmd2 {
        ParsedSlashCommand::Bookmark { label, pts_ms } => {
            assert_eq!(label.as_deref(), Some("Climax scene"));
            assert_eq!(pts_ms, current_pts);
        }
        _ => panic!("Expected ParsedSlashCommand::Bookmark"),
    }

    // Uppercase without label
    let cmd3 = SlashCommandParser::parse("/BOOKMARK", Some(current_pts))
        .expect("Uppercase /BOOKMARK without label should parse");
    match cmd3 {
        ParsedSlashCommand::Bookmark { label, pts_ms } => {
            assert_eq!(label, None);
            assert_eq!(pts_ms, current_pts);
        }
        _ => panic!("Expected ParsedSlashCommand::Bookmark"),
    }
}

#[test]
fn test_case_insensitive_clip() {
    let cmd1 = SlashCommandParser::parse("/CLIP 00:10 00:25 \"Epic Solo\"", None)
        .expect("Uppercase /CLIP should parse");
    match cmd1 {
        ParsedSlashCommand::Clip {
            start_pts_ms,
            end_pts_ms,
            label,
        } => {
            assert_eq!(start_pts_ms, 10_000);
            assert_eq!(end_pts_ms, 25_000);
            assert_eq!(label.as_deref(), Some("Epic Solo"));
        }
        _ => panic!("Expected ParsedSlashCommand::Clip"),
    }

    let cmd2 = SlashCommandParser::parse("/Clip 30 60", None)
        .expect("Titlecase /Clip should parse");
    match cmd2 {
        ParsedSlashCommand::Clip {
            start_pts_ms,
            end_pts_ms,
            label,
        } => {
            assert_eq!(start_pts_ms, 30_000);
            assert_eq!(end_pts_ms, 60_000);
            assert_eq!(label, None);
        }
        _ => panic!("Expected ParsedSlashCommand::Clip"),
    }
}

#[test]
fn test_case_insensitive_sync_status() {
    let cmd1 = SlashCommandParser::parse("/SYNC-STATUS", None)
        .expect("Uppercase /SYNC-STATUS should parse");
    assert_eq!(cmd1, ParsedSlashCommand::SyncStatus);

    let cmd2 = SlashCommandParser::parse("/Sync-Status", None)
        .expect("Titlecase /Sync-Status should parse");
    assert_eq!(cmd2, ParsedSlashCommand::SyncStatus);

    let cmd3 = SlashCommandParser::parse("/sYnC-sTaTuS", None)
        .expect("Mixed case /sYnC-sTaTuS should parse");
    assert_eq!(cmd3, ParsedSlashCommand::SyncStatus);
}

#[test]
fn test_malformed_poll_returns_err_without_panic() {
    // Empty command arguments
    let err1 = SlashCommandParser::parse("/POLL", None);
    assert!(matches!(err1, Err(ChatError::InvalidCommand(_))));

    let err2 = SlashCommandParser::parse("/Poll    ", None);
    assert!(matches!(err2, Err(ChatError::InvalidCommand(_))));

    // Single option or invalid syntax
    let err3 = SlashCommandParser::parse("/poll \"Single item\"", None);
    assert!(matches!(err3, Err(ChatError::InvalidCommand(_))));

    let err4 = SlashCommandParser::parse("/POLL OnlyQuestionWithoutPipeOrQuotes", None);
    assert!(matches!(err4, Err(ChatError::InvalidCommand(_))));

    let err5 = SlashCommandParser::parse("/Poll Question |", None);
    assert!(matches!(err5, Err(ChatError::InvalidCommand(_))));

    let err6 = SlashCommandParser::parse("/POLL | |", None);
    assert!(matches!(err6, Err(ChatError::InvalidCommand(_))));
}

#[test]
fn test_malformed_seek_and_clip_return_err_without_panic() {
    // Missing time parameter for seek
    let err1 = SlashCommandParser::parse("/SEEK", None);
    assert!(matches!(err1, Err(ChatError::InvalidCommand(_))));

    let err2 = SlashCommandParser::parse("/Seek    ", None);
    assert!(matches!(err2, Err(ChatError::InvalidCommand(_))));

    // Invalid time strings
    let err3 = SlashCommandParser::parse("/SEEK not_a_time", None);
    assert!(matches!(err3, Err(ChatError::InvalidCommand(_))));

    // Clip with missing end time or start > end
    let err4 = SlashCommandParser::parse("/CLIP 10", None);
    assert!(matches!(err4, Err(ChatError::InvalidCommand(_))));

    let err5 = SlashCommandParser::parse("/CLIP 50 20", None);
    assert!(matches!(err5, Err(ChatError::InvalidCommand(_))));
}

#[test]
fn test_unknown_command_case_returns_err_without_panic() {
    let err1 = SlashCommandParser::parse("/UNKNOWN_CMD 123", None);
    assert!(matches!(err1, Err(ChatError::InvalidCommand(_))));

    let err2 = SlashCommandParser::parse("/NOT_A_COMMAND", None);
    assert!(matches!(err2, Err(ChatError::InvalidCommand(_))));
}
