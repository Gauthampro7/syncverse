# Dispatch: Explorer Survey 1 (Network Wire Protocol & Peer State)

## Objective
Investigate `crates/syncplay-sync` and related crates (`syncplay-common`, `syncplay-core`) to analyze:
1. Current UDP wire packet structure, serialization/deserialization, and network transport protocol.
2. How peer state (connected peers, RTT, clock offset) is currently stored and tracked.
3. How to extend the wire protocol to exchange loaded media metadata:
   - filename (basename without full path)
   - duration_ms
   - file_size_bytes
4. How to extend the wire protocol to exchange peer readiness state (`Ready` vs `Not Ready`).
5. How barrier unpause or play commands currently work across peers and where the unpause gate hooks in.

## Input Information
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (read section `## 2026-10-26T11:12:06Z`)
- `/home/gautham/syncplay-alt/crates/syncplay-sync`
- `/home/gautham/syncplay-alt/crates/syncplay-common`
- `/home/gautham/syncplay-alt/crates/syncplay-core`

## Output Requirements
Produce a comprehensive report in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/handoff.md`.
Include:
- Architecture analysis
- Existing packet types & byte layout / serialization
- Proposed wire packet changes for media metadata & readiness
- Integration touchpoints with sync engine
- Risks, backwards compatibility, and test strategy


## 2026-10-26T11:15:20Z
You are Explorer 1 for Phase 5 Survey.
Your identity: explorer_survey_1
Your working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/
Read your dispatch assignment in /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/DISPATCH.md and the authoritative user request in /home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md (under section `## 2026-10-26T11:12:06Z`).

Investigate the network wire protocol and peer state in crates/syncplay-sync, crates/syncplay-common, and crates/syncplay-core.
Analyze how to extend the wire packet protocol for:
1. Peer media metadata (filename, duration_ms, file_size_bytes)
2. Peer readiness state (Ready vs Not Ready)
3. Barrier unpause gate integration

Produce your comprehensive handoff report at /home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/handoff.md and message back when complete.
