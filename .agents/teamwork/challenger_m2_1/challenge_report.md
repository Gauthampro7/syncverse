# Adversarial Challenge Report: Pi Framework Stress Testing (Milestone M2)

## Challenge Summary

**Overall risk assessment**: CRITICAL

The Pi Framework deliverables (`AGENTS.md`, `.pi/agents/*.md`, `.pi/skills/*`, `.pi/workflows/*`) provide strong architectural foundations and pass existing baseline tests. However, adversarial empirical stress testing revealed critical specification errors and dependency audit evasion vulnerabilities that will cause downstream implementation failure in Milestone M3 if left unaddressed.

Specifically:
1. **[CRITICAL] Binary Framing Bug in `storage_auditor.md`**: Line 32 defines `record_total_len = length of body + 4`, directly contradicting `openspec/specs/smart-chat/spec.md` line 54 and `test_smart_chat.py`. Empirical test execution proved this causes 100% recovery failure and database corruption.
2. **[HIGH] Dependency Audit Script Evasion**: The audit script in `.pi/skills/zero-dependency-audit/SKILL.md` fails to detect prohibited dependencies when word suffixes (`libavcodec`, `sqlite3`), package aliases (`my-codec = { package = "ffmpeg-next" }`), or bracketed syntax (`[dependencies.sqlite3]`) are used in `Cargo.toml`. Furthermore, multiple prohibited crates (`diesel`, `hound`, `image`, `imageproc`, `libswscale`, `libswresample`) are omitted from `BANNED_PATTERNS`, allowing them to bypass `Cargo.lock` checks undetected.
3. **[MEDIUM] Inconsistent Banned Crate Lists across Prompts**: `spec_implementer.md` line 15 and `storage_auditor.md` line 12 list truncated subsets of the prohibited crates defined in `AGENTS.md`.

---

## Challenges

### [Critical] Challenge 1: `storage_auditor.md` Wire Framing Desynchronization

- **Assumption challenged**: The agent prompt for storage auditing accurately specifies the binary chat log wire layout and allows interoperability with the recovery scanner.
- **Attack scenario**: An agent implementing `syncplay-chat` follows `.pi/agents/storage_auditor.md` line 32:
  ```markdown
  record_total_len: u32 (4 bytes, little-endian) = length of body + 4.
  ```
  When the recovery scanner (`ChatRecoveryScanner`) in `test_smart_chat.py` attempts to parse records written under this definition:
  1. For small records, `record_total_len` drops below 48, triggering false-candidate rejection (`total_len < 48`).
  2. For larger records, reading `record_bytes = data[offset : offset + total_len]` slices only `len(body) + 4` bytes instead of the full `len(body) + 10` bytes (missing the 2-byte sync word and 4-byte length header). The body extraction `record_bytes[6:-4]` becomes corrupted by 6 bytes, CRC32 verification fails, and the scanner advances byte-by-byte into the record body, flagging all subsequent records as corrupted chunks.
- **Blast radius**: Complete breakdown of storage crash recovery in `syncplay-chat`. Zero valid records recovered, authoritative recovery tests fail, and log files become permanently unreadable.
- **Empirical Proof**: Executing `tests/empirical_challenge/test_pi_adversarial_stress.py` (Challenge 2) demonstrates that a 2-record binary log written with `body + 4` yields 0 recovered records, 1 torn write, and 3 corrupted chunks.
- **Mitigation**: Update `.pi/agents/storage_auditor.md` line 32 to conform with `openspec/specs/smart-chat/spec.md` line 54 and `test_smart_chat.py`:
  ```markdown
  record_total_len: u32 (4 bytes, little-endian) = total record length in bytes (sync word 2B + length 4B + body + crc32 4B = length of body + 10, >= 48).
  ```

---

### [High] Challenge 2: Audit Script Dependency Evasion in `zero-dependency-audit/SKILL.md`

- **Assumption challenged**: The automated audit script in `.pi/skills/zero-dependency-audit/SKILL.md` strictly blocks any inclusion or vendoring of prohibited multimedia and database crates.
- **Attack scenario**:
  1. **Word-Suffix Evasion in `Cargo.toml`**: The audit script checks `grep -rE "^\s*${pattern}\b"`. In regex, `\b` is a word boundary. For pattern `"libav"`, it fails to match `libavcodec`, `libavformat`, or `libavutil` because `codec` starts with a word character (`\w`). For pattern `"sqlite"`, it fails to match `sqlite3` because `3` is a word character (`\w`). Furthermore, `libswscale` and `libswresample` do not contain `libav` at all.
  2. **Package Aliasing**: An agent writes `my_codec = { package = "ffmpeg-next", version = "0.1" }` or `[dependencies.sqlite3]`. Neither matches the regex `^\s*${pattern}\b`.
  3. **Omitted Crates in `Cargo.lock`**: `diesel`, `hound`, `image`, `imageproc`, `libswscale`, `libswresample` are absent from `BANNED_PATTERNS` in lines 82–102 of the skill. When present in `Cargo.lock`, `grep -E "^name = \"${pattern}"` never tests for them, allowing full vendoring without CI rejection.
- **Blast radius**: Accidental or deliberate bypass of the "Zero-External-Codec Invariant" in CI or autonomous agent runs, violating Requirement R2.
- **Empirical Proof**: Executed `tests/empirical_challenge/test_pi_adversarial_stress.py` (Challenge 1) showing 14 distinct bypass cases that succeed undetected against the provided script.
- **Mitigation**:
  1. Expand `BANNED_PATTERNS` in `.pi/skills/zero-dependency-audit/SKILL.md` to include all crates from `AGENTS.md`: `diesel`, `hound`, `image`, `imageproc`, `libswscale`, `libswresample`, `libavcodec`, `libavformat`, `libavutil`, `sqlite3`.
  2. In `Cargo.toml` scanning, inspect dependencies using `cargo tree` or grep for `package = "..."` and bracketed table headers `\[(dependencies|build-dependencies|dev-dependencies)\.`.
  3. Replace `^\s*${pattern}\b` with a pattern that matches crate prefixes and package clauses: `(^\s*${pattern}|package\s*=\s*"${pattern}|\[.*${pattern})`.

---

### [Medium] Challenge 3: Incomplete Banned Lists in Agent Prompts

- **Assumption challenged**: Agent prompts provide self-contained, exhaustive constraints without requiring cross-referencing against other files.
- **Attack scenario**: An agent assigned as `spec_implementer` reads only `.pi/agents/spec_implementer.md`. Line 15 omits `diesel`, `hound`, `image`, and `imageproc`. The implementer might use `image` for RGBA buffer manipulation or `hound` for audio buffer testing, believing it is permitted under its prompt. Similarly, `storage_auditor.md` line 12 mentions only `SQLite, RocksDB, Sled, Heed`, omitting `diesel` and `sqlx`.
- **Blast radius**: Agent confusion, introducing forbidden crates during early implementation phases that are later caught late in CI.
- **Mitigation**: Synchronize the banned crate lists in `.pi/agents/spec_implementer.md` line 15 and `.pi/agents/storage_auditor.md` line 12 with the authoritative blacklist in `AGENTS.md` lines 21–22.

---

## Stress Test Results

| Scenario | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|
| `libavcodec` in `Cargo.toml` without lockfile | Detected & Rejected | Bypassed detection (`\b` fails on `codec`) | **FAIL** |
| `sqlite3` in `Cargo.toml` without lockfile | Detected & Rejected | Bypassed detection (`\b` fails on `3`) | **FAIL** |
| `my_codec = { package = "ffmpeg-next" }` alias | Detected & Rejected | Bypassed detection (line does not start with pattern) | **FAIL** |
| `[dependencies.sqlite3]` bracketed table | Detected & Rejected | Bypassed detection (line starts with `[`) | **FAIL** |
| `diesel` in `Cargo.lock` | Detected & Rejected | Bypassed detection (`diesel` absent from `BANNED_PATTERNS`) | **FAIL** |
| `hound` in `Cargo.lock` | Detected & Rejected | Bypassed detection (`hound` absent from `BANNED_PATTERNS`) | **FAIL** |
| `image` in `Cargo.lock` | Detected & Rejected | Bypassed detection (`image` absent from `BANNED_PATTERNS`) | **FAIL** |
| `imageproc` in `Cargo.lock` | Detected & Rejected | Bypassed detection (`imageproc` absent from `BANNED_PATTERNS`) | **FAIL** |
| `storage_auditor.md` line 32 (`body + 4`) DB recovery | 2 records recovered, 0 corruptions | 0 records recovered, 3 corrupt chunks flagged | **FAIL** |
| `bitstream-vector-testing` synthetic EBML vector | Exactly 48 bytes, parses DocType='matroska' | Exactly 48 bytes, parsed DocType='matroska', v4 | **PASS** |
| `bitstream-vector-testing` synthetic MP4 vector | Exactly 132 bytes, parses ftyp/moov/mvhd | Exactly 132 bytes, 3 boxes parsed cleanly | **PASS** |
| `network-sync-simulation` discrete PI PLL loop | Converges <15ms in <=20s, no overshoot <-15ms | Converged at t=12.0s, overshoot=False | **PASS** |
| OpenSpec change proposals dependency graph | Directed Acyclic Graph (0 cycles) | Valid topological DAG (6 proposals, 0 cycles) | **PASS** |

---

## Unchallenged Areas

- **Full Workspace Compilation (`cargo check --workspace`)**: Rust crate scaffolding in `crates/` is not yet created (scheduled for Milestone M3). Workspace compilation could not be evaluated at this milestone.
- **Physical Multi-Peer WebRTC / UDP Socket Streaming**: Pure-Rust socket streaming across live WAN nodes is deferred to runtime implementation in Milestone M4.

---

## Verdict & Actionable Remediation

**Verdict**: `REQUEST_CHANGES`

Downstream worker agents must remediate the following 3 items before Milestone M2 can be approved:
1. **Fix `storage_auditor.md` line 32**: Correct `record_total_len` definition to total record length (`length of body + 10`, $\ge 48$).
2. **Update `zero-dependency-audit/SKILL.md`**:
   - Add missing crates to `BANNED_PATTERNS`: `diesel`, `hound`, `image`, `imageproc`, `libswscale`, `libswresample`, `libavcodec`, `libavformat`, `libavutil`, `sqlite3`.
   - Update regex scanning logic to catch package aliases and bracketed tables.
3. **Synchronize `spec_implementer.md` and `storage_auditor.md`**: Ensure banned dependency lists match `AGENTS.md` lines 21–22 in full.
