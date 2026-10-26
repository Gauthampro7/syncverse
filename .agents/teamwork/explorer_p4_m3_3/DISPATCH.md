# DISPATCH — explorer_p4_m3_3

## Mission
You are `explorer_p4_m3_3`, exploring the Watch-Party Sidebar, Smart Chat, Native File Dialog (`rfd`), and Application CLI Integration for Milestone 3 (Flawless Desktop Graphical Player UI).
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_3`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z` R1 requirements)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Codebase files:
   - `crates/syncplay-ui/Cargo.toml`
   - `crates/syncplay-ui/src/lib.rs`
   - `crates/syncplay-ui/src/sidebar.rs`
   - `crates/syncplay-chat/src/lib.rs`
   - `crates/syncplay-core/src/main.rs`
   - `crates/syncplay-core/src/coordinator.rs`

## Investigation Scope
1. **Watch-Party Sidebar (Collapsible Right Panel)**:
   - Collapse / Expand toggle button.
   - Room Members list (usernames, local user highlighted).
   - Connection RTT indicator with color coding:
     * Green: In-sync ($< 15\text{ms}$ phase error)
     * Amber: Steering ($15\text{ms} - 1500\text{ms}$ micro-speed PLL)
     * Red: Seeking / Out-of-sync ($> 1500\text{ms}$)
2. **Real-Time Smart Chat Integration**:
   - Scrollable chat message history showing:
     * Sender username
     * Dual timestamps: $t_{\text{wall}}$ (`HH:MM:SS`) and $t_{\text{media}}$ (`[MM:SS]`)
     * Message body text
   - Chat input text box with Enter-to-send.
   - Slash-command parsing & dispatch:
     * `/seek <timestamp>` (e.g. `/seek 01:23`)
     * `/bookmark <label>`
     * `/clip <duration>`
     * `/poll <question>`
     * `/sync-status`
3. **Native File Selection (`rfd`) & Drag-and-Drop**:
   - Open File button triggering native system file dialog via `rfd::FileDialog`:
     * File extensions filter: `["mkv", "webm", "mp4", "mov", "avi", "ts", "flv", "ogg", "ogv", "mp3", "flac", "wav", "aac", "opus"]`.
     * Non-blocking dialog spawn (using thread or async).
   - Drag-and-drop file loading:
     * Handling `ctx.input(|i| i.raw.dropped_files.clone())`.
   - Recent files history in menu bar or open button dropdown.
4. **Integration with `syncplay-core/src/main.rs`**:
   - Running GUI by default: `syncplay-alt [FILE]` opens the desktop graphical window.
   - Preserving `--headless` and `--terminal` flags for automated tests and TTY.

## Output Requirements
Document your component design, message flow, and draft code in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_3/handoff.md`.
Send a completion message to the orchestrator.

## 2026-10-25T23:51:41Z
You are `explorer_p4_m3_3`. Read `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_3/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically R1 requirements).
Investigate and design:
1. Watch-party sidebar (collapsible right panel) with room members, RTT, sync health badge (Green, Amber, Red), real-time smart chat display with dual timestamps ($t_{wall}, t_{media}$), and slash commands (`/seek`, `/bookmark`, `/clip`, `/poll`, `/sync-status`).
2. Native file dialog via `rfd` (XDG Desktop Portal on Fedora Linux, Win32 on Windows), drag-and-drop file loading into window, and recent files history.
3. Default desktop GUI launch in `crates/syncplay-core/src/main.rs`, preserving `--headless` and `--terminal` flags.
Write your architecture and prototype code to `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_3/handoff.md` and send a message to the orchestrator.
