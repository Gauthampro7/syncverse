---
name: intent-verification-harness
description: "Executes the Pi Framework Intent Verification Harness, validates the 8 foundational architectural intents, and prevents intent drift across autonomous coding agent runs."
---

# Intent Verification Harness Skill (`.pi/skills/intent-verification-harness`)

## Overview
This skill guides agents in running and interpreting the **Intent Verification Harness** (`scripts/intent_harness.py`). The harness bridges high-level user and architectural intents with low-level test fixtures and static analysis rules defined in `.pi/harness/intent_matrix.json`.

---

## When to Use This Skill
- Before proposing or committing any code changes to ensure no intent invariants have drifted.
- When adding new capabilities or change proposals to update `.pi/harness/intent_matrix.json`.
- When conducting victory audits or multi-gate milestone reviews.

---

## Execution Instructions

### 1. Run the Intent Harness
```bash
python3 scripts/intent_harness.py
```
Or via the shell wrapper:
```bash
./scripts/verify_intent.sh
```

To run a specific intent:
```bash
python3 scripts/intent_harness.py --intent INTENT-01
```

To list all registered intents:
```bash
python3 scripts/intent_harness.py --list
```

### 2. Verify Output & Invariant Compliance
The harness will evaluate all 8 core intents:
- `INTENT-01`: Universal Media Compatibility & Zero Monolithic Player / Database Wrappers (`./scripts/audit_dependencies.sh`)
- `INTENT-02`: High-RTT Clock Phase Alignment (`test_sync_engine.py`)
- `INTENT-03`: Barrier-Synchronized Unpause (`test_sync_engine.py`)
- `INTENT-04`: Crash-Resilient Smart Chat Log (`test_smart_chat.py`)
- `INTENT-05`: Desktop Graphical UI Architecture & Lock-Free Threading (`test_ui_renderer.py`)
- `INTENT-06`: Q8 Fixed-Point Color Pipeline (`test_demux_codecs_clock.py`)
- `INTENT-07`: Container Demuxing Bounds Safety (`test_demux_codecs_clock.py`)
- `INTENT-08`: OpenSpec Traceability & Anti-Drift (`cargo test --workspace`)

### 3. Inspect the JSON Report
```bash
cat .pi/harness/intent_report.json
```
Ensure `"compliance_score_percent": 100.0` and `"status": "PASS"`.

### 4. Updating the Intent Matrix
When new user intents or architectural contracts are established, append them to `.pi/harness/intent_matrix.json` following the schema:
```json
{
  "id": "INTENT-XX",
  "name": "...",
  "description": "...",
  "openspec_ref": "...",
  "capabilities": ["..."],
  "verification": {
    "command": "...",
    "type": "...",
    "expected_exit_code": 0
  },
  "invariants": ["..."]
}
```
