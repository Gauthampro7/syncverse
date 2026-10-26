# Dispatch: explorer_p3_survey_1

## Identity
- Role: Technical Explorer (Survey R1 & R2)
- Working Directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_1
- Authoritative User Request: /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md

## Mission
Conduct an exhaustive technical survey of:
1. R1: Fedora Desktop Application (`syncplay-alt.desktop`) and Interactive Terminal/OSD Presentation Mode (`play <FILE>`).
2. R2: Pure-Rust Linux Audio Output (zero external C/C++ audio frameworks) & Resampling Engine (`AudioFramePCM`, linear resampler 44.1k <-> 48k, clock pacing, underrun defense).

## Output
Write your findings to `survey_report.md` and complete with `handoff.md` in your working directory.

## 2026-10-25T15:26:25Z
You are explorer_p3_survey_1, a technical explorer for Phase 3 of Syncplay-Alt.
Your working directory is /home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_1.
The workspace root is /home/gautham/syncplay-alt.

Read your dispatch instructions in:
/home/gautham/syncplay-alt/.agents/teamwork/explorer_p3_survey_1/DISPATCH.md
and the authoritative user request in:
/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (specifically the latest request under ## 2026-10-25T15:21:44Z).

Your mission is to explore and survey:
1. R1: Fedora Desktop Application (`syncplay-alt.desktop`) and Interactive Terminal/OSD Presentation Mode (`play <FILE>`).
   - What needs to be in `~/.local/share/applications/syncplay-alt.desktop` (Exec, MIME types, modern icon integration, validation).
   - How terminal interactive raw mode, keyboard navigation (Space, Left/Right, Shift+Left/Right, [, ], M, Up/Down, T/C, S, /, Q/Esc) and 60fps presentation loop should be implemented without external terminal/UI engines.
   - ANSI 24-bit TrueColor / box-drawing / half-block video rendering, real-time scrubber progress bar, timecode HUD (`format_timecode_hud`), tri-color sync health badge, and floating bullet comments.
2. R2: Pure-Rust Linux Audio Output & Resampling Engine.
   - Zero-external-dependency audio sink for Linux (native ALSA PCM via /dev/snd/pcm* or unbuffered raw PCM audio pipe to standard output / sound server like pw-cat / aplay or standard Linux audio interfaces in pure Rust).
   - Audio clock pacing synchronized with `MasterClock` presentation time.
   - Zero-allocation audio buffer underrun protection and linear sample rate conversion (e.g. 44.1 kHz <-> 48 kHz).
   - How `syncplay-codecs` `AudioFramePCM` and `syncplay-clock` 128-byte cache-line aligned SPSC ring buffer connect to audible playback.

Inspect existing code in `crates/syncplay-ui`, `crates/syncplay-core`, `crates/syncplay-clock`, `crates/syncplay-codecs`.
Write a comprehensive report to `survey_report.md` in your working directory and deliver a complete `handoff.md`.
Communicate back with send_message to orchestrator_3 (my conversation ID is ecd27bad-3488-46aa-ba26-8b0b7cecf0eb).
