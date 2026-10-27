# E2E Test Infra: Syncverse Phase 5 (Watch-Party Social & Auto-Update Polish)

## Test Philosophy
- Opaque-box, requirement-driven. Derived from `ORIGINAL_REQUEST.md` and user-facing specifications, not internal private structures.
- Methodology: Category-Partition + Boundary Value Analysis (BVA) + Pairwise Combinatorial Testing + Real-World Workload Scenarios.

## Feature Inventory
| # | Feature | Source (requirement) | Tier 1 | Tier 2 | Tier 3 |
|---|---------|---------------------|:------:|:------:|:------:|
| 1 | GitHub Releases API Update Querying | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ |
| 2 | Semver Parsing & Comparison | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ |
| 3 | Auto-Update Notification Banner & Actions | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ |
| 4 | Help Menu "Check for Updates..." | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ |
| 5 | Peer Media Wire Packet Metadata | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ |
| 6 | Peer Readiness Wire Packet State | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ |
| 7 | Negative Opcode Validation Update | Survey Explorer 1 | 5 | 5 | ✓ |
| 8 | Persistent Readiness vs Barrier Preroll | Survey Explorer 1 | 5 | 5 | ✓ |
| 9 | Media Status Broadcast on Open/Change | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ |
| 10 | Room Activity Announcements Formatting | ORIGINAL_REQUEST §R4 | 5 | 5 | ✓ |
| 11 | Seek Announcement Debouncing | Survey Explorer 3 | 5 | 5 | ✓ |
| 12 | Room Announcements Chat Storage & FTS | ORIGINAL_REQUEST §R4 | 5 | 5 | ✓ |
| 13 | Sidebar Peer Media Display | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ |
| 14 | File Match Diagnostics Badge | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ |
| 15 | "Ready to Watch" Toggle & Badges | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ |
| 16 | Strict Playback Unpause Gate | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ |
| 17 | Host "Force Play" Bypass | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ |
| 18 | 72-Byte Snapshot Invariant | Survey Explorer 2 | 5 | 5 | ✓ |

## Test Architecture
- Test runner: `cargo test --test <test_name>` or standard integration tests in `tests/`.
- Test case format: Rust integration tests exercising the public API, wire packets, and command/event pipelines.
- Directory layout: `tests/` and crate-level `tests/`.

## Real-World Application Scenarios (Tier 4)
| # | Scenario | Features Exercised | Complexity |
|---|----------|--------------------|------------|
| 1 | Normal 3-Peer Watch Party Playback | F5, F6, F9, F10, F13, F14, F15, F16 | High |
| 2 | Late-Joining Unready Peer Blocks Unpause | F5, F6, F13, F15, F16 | High |
| 3 | Host Forces Play with AFK Peer | F6, F15, F16, F17, F10 | Medium |
| 4 | File Mismatch Diagnostic Warning | F5, F9, F13, F14 | Medium |
| 5 | Update Check & UI Banner Dismissal | F1, F2, F3, F4 | Medium |

## Coverage Thresholds
- Tier 1: ≥5 per feature
- Tier 2: ≥5 per feature (where boundaries exist)
- Tier 3: pairwise coverage of major feature interactions
- Tier 4: ≥5 realistic application scenarios
