# Workflow: Intent Verification & Anti-Drift Audit (`.pi/workflows/intent-verification-workflow.md`)

## Purpose
Enforces intent alignment before any PR, change proposal, or milestone completion is merged. Guarantees that passing low-level unit tests does not mask architectural regression or user intent drift.

---

## Workflow Steps

```
[Start Work] ──► [Implement Feature] ──► [Local Unit Tests]
                                                │
                                                ▼
[Milestone Approved] ◄── [Audit Passed] ◄── [Intent Harness Execution]
                                                │ (if failed)
                                                ▼
                                         [Remediate Drift]
```

### Step 1: Pre-Implementation Intent Baseline
Before writing code, verify that the current workspace satisfies all existing intents:
```bash
python3 scripts/intent_harness.py
```

### Step 2: Code Implementation & Local Testing
Write feature code conforming to `AGENTS.md` and `#![deny(warnings)]`:
```bash
cargo check --workspace --all-targets
cargo test --workspace -- --nocapture
```

### Step 3: Intent Verification Gate
Run the intent harness to verify that all 8 core architectural intents remain 100% compliant:
```bash
./scripts/verify_intent.sh
```

### Step 4: Full End-to-End Suite
Execute the master automated verification script:
```bash
./scripts/verify_phase1.sh
```
