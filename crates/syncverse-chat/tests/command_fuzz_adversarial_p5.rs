//! Adversarial fuzzing and edge-case stress test suite for slash command parsing in `syncverse-chat`.
//!
//! Stress-tests:
//! 1. Arbitrary Unicode, non-ASCII characters, and emojis across `/seek`, `/bookmark`, and `/poll`.
//! 2. Unclosed quotes and whitespace variations in `/poll`.
//! 3. Pipe syntax with trailing/empty elements and multiple leading spaces.
//! 4. Verification of arithmetic multiplication overflow in `parse_time_to_ms` using `std::panic::catch_unwind`.

#![deny(warnings)]

use syncverse_chat::{ParsedSlashCommand, SlashCommandParser};

#[test]
fn test_slash_command_unicode_and_emojis() {
    // Emojis and Unicode in bookmark labels
    let res = SlashCommandParser::parse("/bookmark 🎬 Interstellar docking sequence 🚀", Some(123456))
        .expect("Emoji bookmark should parse successfully");
    match res {
        ParsedSlashCommand::Bookmark { label, pts_ms } => {
            assert_eq!(label, Some("🎬 Interstellar docking sequence 🚀".to_string()));
            assert_eq!(pts_ms, 123456);
        }
        _ => panic!("Expected ParsedSlashCommand::Bookmark"),
    }

    // Multilingual question and emoji options in /poll
    let res = SlashCommandParser::parse(
        r#"/poll "Quel est votre film préféré ? 🍿" "インターステラー 🚀" "ブレードランナー 🤖""#,
        None,
    )
    .expect("Multilingual poll should parse successfully");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Quel est votre film préféré ? 🍿");
            assert_eq!(options, vec!["インターステラー 🚀", "ブレードランナー 🤖"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }
}

#[test]
fn test_slash_command_unclosed_quotes_resilience() {
    // Unclosed quote with only one token returns Err without panicking
    let res = SlashCommandParser::parse(r#"/poll "Unclosed Question"#, None);
    assert!(res.is_err(), "Unclosed question should safely return Err");

    // Unclosed quote on final option: parser drops trailing unclosed option
    let res = SlashCommandParser::parse(
        r#"/poll "Question" "Option 1" "Option 2"#,
        None,
    )
    .expect("Should parse with unclosed trailing option dropped");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Question");
            // "Option 2" was dropped because closing quote was missing
            assert_eq!(options, vec!["Option 1"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }
}

#[test]
fn test_slash_command_pipe_syntax_leading_whitespace_behavior() {
    // Standard pipe syntax
    let res = SlashCommandParser::parse("/poll Best Framework | Tokio | Rayon | Async-std", None)
        .expect("Standard pipe poll should parse");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Best Framework");
            assert_eq!(options, vec!["Tokio", "Rayon", "Async-std"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }

    // Pipe syntax with extra spaces between slash and poll:
    // Safely slices argument text after leading whitespace and command name.
    let res = SlashCommandParser::parse("/   poll Best Framework | Tokio | Rayon", None)
        .expect("Leading space pipe poll should parse");
    match res {
        ParsedSlashCommand::Poll { question, options } => {
            assert_eq!(question, "Best Framework");
            assert_eq!(options, vec!["Tokio", "Rayon"]);
        }
        _ => panic!("Expected ParsedSlashCommand::Poll"),
    }
}

#[test]
fn test_seek_time_overflow_panic_demonstration() {
    // In `parse_time_to_ms`, hours > 87_600 returns ChatError::InvalidCommand gracefully without panicking.
    let result = std::panic::catch_unwind(|| {
        SlashCommandParser::parse("/seek 9999999999999999:00:00", None)
    });

    assert!(
        result.is_ok(),
        "Expected /seek with massive hours to handle gracefully without panic"
    );
    let parse_res = result.unwrap();
    assert!(
        parse_res.is_err(),
        "Expected Err(ChatError::InvalidCommand) on massive hours input, got: {:?}",
        parse_res
    );
}

#[test]
fn test_seek_minutes_overflow_panic_demonstration() {
    // In `parse_time_to_ms` MM:SS branch, mins > 5_256_000 returns ChatError::InvalidCommand gracefully without panicking.
    let result = std::panic::catch_unwind(|| {
        SlashCommandParser::parse("/seek 9999999999999999:00", None)
    });

    assert!(
        result.is_ok(),
        "Expected /seek with massive minutes to handle gracefully without panic"
    );
    let parse_res = result.unwrap();
    assert!(
        parse_res.is_err(),
        "Expected Err(ChatError::InvalidCommand) on massive minutes input, got: {:?}",
        parse_res
    );
}
