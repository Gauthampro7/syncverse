# Handoff Report: Milestone 1 Adversarial Empirical Challenge

**Agent**: `challenger_m1_2` (teamwork_preview_challenger)  
**Roles**: critic, specialist  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_m1_2/`  
**Milestone**: Milestone 1 (OpenSpec Architecture & Specification Preview)  
**Verdict**: **`REQUEST_CHANGES`**  
**Handoff Type**: Hard (Task Complete)

---

## 1. Observation

Direct observations obtained by executing empirical test harnesses in `/home/gautham/syncplay-alt/tests/empirical_challenge/` against `openspec/specs/sync-engine/spec.md`, `openspec/specs/smart-chat/spec.md`, and `openspec/specs/ui-renderer/spec.md`:

### 1.1 Sync Engine PI Controller Convergence Contradiction
- In `openspec/specs/sync-engine/spec.md` (lines 111–125):
  - Parameters: $K_p = 0.05\text{ s}^{-1}, K_i = 0.000625\text{ s}^{-2}, \Delta t = 0.2\text{ s}, \Delta r_{\text{max}} = 0.04$.
  - Scenario claim: *"initial phase error $e(0) = +450\text{ ms}$... phase error $e(t)$ monotonically decays into the $|e| < 15\text{ ms}$ deadband within $20\text{ seconds}$"*.
  - Execution via `python3 tests/empirical_challenge/test_sync_engine.py`:
    ```
    Initial error: 450.00 ms
    Error at t=20.0s: 135.24 ms
    Error at t=25.0s: 90.76 ms
    Time entered < 15ms deadband: NEVER (within 25s)
    Actual time to enter < 15ms deadband: ~68.0 seconds
    ```
  - Quoted code result: At $t = 20.0\text{ s}$, remaining phase error is $135.24\text{ ms}$, failing the scenario acceptance threshold by $900\%$.

### 1.2 NTP Sliding-Window Filter Zero-Dispersion Latch-Up & Reroute Lock-Out
- In `openspec/specs/sync-engine/spec.md` (lines 50–55):
  - Formula: $D = \sum_{i=0}^{W-1} \frac{|s_{(i)}.\text{offset} - s_{(0)}.\text{offset}|}{2^{i+1}}$
  - Rejection condition: $s_{\text{new}}.\text{rtt} > 2.0 \times \text{median}(\text{rtt}) \lor |s_{\text{new}}.\text{offset} - \hat{\theta}| > 3 \times D$.
  - Execution via `python3 tests/empirical_challenge/test_sync_engine.py`:
    - After 8 identical initial samples, $D = 0.0\text{ ns}$.
    - Subsequent legitimate $+10\text{ ms}$ offset drift probe: rejected (`|s_new.offset - theta_hat| = 10.0 ms vs 3*D = 0.0 ms`).
    - 50 consecutive valid clock update probes tested: `50 / 50 rejected (100% lock-out)`.
    - Path reroute test where RTT jumped from $100\text{ ms}$ to $210\text{ ms}$: `50 / 50 rejected (100% lock-out)`.

### 1.3 Barrier Unpause Deadline Expiration on 400ms RTT Links
- In `openspec/specs/sync-engine/spec.md` (lines 132–134):
  - Formula: $T_{\text{target\_server}} = t_{\text{server\_now}} + \max_i(\text{RTT}_i) + 200\text{ ms}$.
  - Execution via `python3 tests/empirical_challenge/test_sync_engine.py`:
    - On a link with $\max(\text{RTT}) = 400\text{ ms}$ ($1.5 \times \text{RTT} = 600\text{ ms}$ network transit):
    - With $50\text{ ms}$ pre-roll: Commit arrives at $t = 650\text{ ms}$ vs target $600\text{ ms}$ (Margin: $-50\text{ ms}$).
    - With $100\text{ ms}$ pre-roll: Commit arrives at $t = 700\text{ ms}$ vs target $600\text{ ms}$ (Margin: $-100\text{ ms}$).

### 1.4 Smart Chat False-Sync `0xAA55` Database Truncation
- In `openspec/specs/smart-chat/spec.md` (lines 76–79):
  - Rule: *"Torn Write at EOF: If current_offset + record_total_len > file_len or reading returns EOF mid-record, the scanner MUST treat the file as truncated at current_offset, preserve all preceding valid records, and salvage the database."*
  - Execution via `python3 tests/empirical_challenge/test_smart_chat.py`:
    - Test log created with 10 records. Record 2 had corrupted CRC and payload containing `[0xAA, 0x55, 0xFF, 0xFF, 0xFF, 0x00]`. Records 3–9 were completely valid.
    - Scanner evaluated `current_offset + total_len > file_len` as TRUE.
    - Scanner aborted recovery, returning only 2 records.
    - **7 out of 9 valid records permanently discarded**.

### 1.5 FTS CJK Unsegmented Tokenization & Single Ideograph Filtering
- In `openspec/specs/smart-chat/spec.md` (lines 90–95):
  - Execution via `python3 tests/empirical_challenge/test_smart_chat.py`:
    - CJK sentence `"这是一段非常精彩的吉他独奏"` tokenized into 1 token: `['这是一段非常精彩的吉他独奏']`.
    - Query for `"吉他"` yielded match: `False`.
    - Single ideographs `"猫 狗"`: discarded by `len < 2` rule (`tokens = []`).

### 1.6 UI Renderer Triple-Buffer Multi-Producer Race & Scrubber Debounce Count
- In `openspec/specs/ui-renderer/spec.md`:
  - Execution via `python3 tests/empirical_challenge/test_ui_renderer.py`:
    - Multi-producer test produced 615 torn snapshot reads out of 3,240 polls.
    - Scrubber debouncing at 10 Hz over 500ms drag produced 5 intermediate seeks + 1 mouse-release seek = 6 seeks (contradicting scenario text "at most 5 commands").
    - Font atlas 2048x2048 RGBA8 requires 16 MB ($106.7\%$ of total 15 MB UI budget).

---

## 2. Logic Chain

1. **Premise 1 (Discrete Control Theory & Observation 1.1)**:
   A proportional controller with $K_p = 0.05\text{ s}^{-1}$ has an open-loop pole at $s = -0.05$, yielding an exponential decay time constant $\tau = 20.0\text{ seconds}$. Decreasing error by a factor of 30 ($450\text{ ms} \to 15\text{ ms}$) requires $\ln(30) \approx 3.4$ time constants ($68\text{ seconds}$). Therefore, claiming convergence within $20\text{ seconds}$ under these exact gains is a mathematical impossibility. A downstream unit test validating this scenario will fail.
2. **Premise 2 (Filter Theory & Observation 1.2)**:
   Dispersion $D$ measures sample variance across the window. In stable, low-jitter intervals, variance approaches 0 ($D \to 0$). The outlier check $|s_{\text{new}}.\text{offset} - \hat{\theta}| > 3 \times D$ evaluates to $| \Delta \theta | > 0$, causing all subsequent probes with non-zero delta to be discarded. Discarding samples prevents the window from updating, trapping the filter in permanent latch-up. Similarly, an unreachability threshold is required when median RTT permanently doubles.
3. **Premise 3 (Network Protocol Timing & Observation 1.3)**:
   A 2-phase commit barrier unpause is a 3-way exchange (Prepare $\to$, Ready $\leftarrow$, Commit $\to$). For a peer with RTT of $400\text{ ms}$, one-way transit is $200\text{ ms}$. The total elapsed network transit alone is $3 \times 200 = 600\text{ ms} = 1.5 \times \text{RTT}$. Adding non-zero decoding and audio priming latency ($t_{\text{preroll}} > 0$) causes the commit message to arrive strictly after $t_{\text{server\_now}} + \text{RTT} + 200\text{ ms}$, resulting in an expired presentation deadline.
4. **Premise 4 (File Recovery Integrity & Observation 1.4)**:
   A 2-byte sync sequence `0xAA55` is only 16 bits of entropy, which will naturally occur in uncompressed payloads, emojis, or corrupted byte streams. If the scanner interprets a large length following a false sync word as a torn write at EOF, it aborts prematurely and discards all subsequent valid records in the file.
5. **Conclusion**:
   Because these flaws exist in the normative `Requirement:` and `Scenario:` definitions, downstream workers implementing Phase 2 code and writing verification tests will either fail verification or bake catastrophic failure modes into production code. Therefore, specifications require revision before implementation begins.

---

## 3. Caveats

- **Scope Boundary**: Observations apply to the mathematical formulas, protocol specifications, and scenario criteria in `openspec/specs/sync-engine/spec.md`, `smart-chat/spec.md`, and `ui-renderer/spec.md`. The remaining crates (`demuxer`, `codecs`, `av-pipeline`) were audited by sibling agents.
- **Operating Assumptions**: Calculations assumed standard network jitter distributions, POSIX monotonic clocks, and x86_64/aarch64 memory layouts.

---

## 4. Conclusion

**Verdict**: **`REQUEST_CHANGES`**

The following concrete modifications are required in `openspec/specs/`:

1. **`openspec/specs/sync-engine/spec.md`**:
   - Update discrete PI controller gains to $K_p = 0.17\text{ s}^{-1}, K_i = 0.0072\text{ s}^{-2}$ (or update scenario convergence time from $20\text{ s}$ to $70\text{ s}$).
   - Add a minimum dispersion floor $D_{\text{effective}} = \max(D, 5.0\text{ ms})$ and a sample watchdog counter (force-insert after 8 consecutive rejections) to the RFC 5905 filter.
   - Update unpause schedule equation to:
     $$T_{\text{target\_server}} = t_{\text{server\_now}} + 1.5 \times \max_i(\text{RTT}_i) + t_{\text{preroll\_budget}} + 100\text{ ms}$$
     with $t_{\text{preroll\_budget}} = 200\text{ ms}$.
2. **`openspec/specs/smart-chat/spec.md`**:
   - Fix recovery rule: a length exceeding file length mid-stream MUST NOT trigger EOF truncation; the scanner must treat it as false sync and continue byte-by-byte scanning until physical EOF is reached.
   - Add CJK n-gram / character segmentation for Unicode blocks `U+4E00`–`U+9FFF`, and exempt CJK single ideographs from the $\ge 2$ character length rule.
3. **`openspec/specs/ui-renderer/spec.md`**:
   - Mandate that `PlayerUiSnapshot` publication follows an SPSC topology with a single Telemetry Aggregator thread.
   - Adjust scrubber debouncing scenario to specify 6 commands (5 throttled intervals + 1 release).
   - Specify single-channel Alpha8 (`R8_UNORM`) font atlas texture.

---

## 5. Verification Method

To independently verify all findings and reproduce every failure mode:

1. **Execute Empirical Test Suite**:
   ```bash
   python3 /home/gautham/syncplay-alt/tests/empirical_challenge/test_sync_engine.py
   python3 /home/gautham/syncplay-alt/tests/empirical_challenge/test_smart_chat.py
   python3 /home/gautham/syncplay-alt/tests/empirical_challenge/test_ui_renderer.py
   ```
2. **Inspect Empirical Challenge Report**:
   ```bash
   cat /home/gautham/syncplay-alt/.agents/teamwork/challenger_m1_2/challenge_report.md
   ```
3. **Invalidation Conditions**:
   - If a mathematical derivation proves that $K_p = 0.05, K_i = 0.000625$ can reduce error from $450\text{ ms}$ to $< 15\text{ ms}$ in $\le 20\text{ s}$ without violating the $\Delta r \le 0.04$ boundary, Challenge 1 is invalidated.
   - If an algorithmic proof shows that $D \to 0$ does not reject updates when $|s_{\text{new}}.\text{offset} - \hat{\theta}| > 3D$, Challenge 2 is invalidated.
   - If a network trace shows a 3-way exchange over a 400ms RTT link completing in $< 600\text{ ms}$ with positive pre-roll time, Challenge 3 is invalidated.
