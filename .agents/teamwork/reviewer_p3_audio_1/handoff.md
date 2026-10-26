# Handoff Report: Reviewer Milestone 2 Audio Resampler

**Agent**: `reviewer_p3_audio_1`  
**Role**: Code Reviewer and Adversarial Critic (`reviewer`, `critic`)  
**Date**: 2026-10-25T16:58:00Z  
**Recipient**: `orchestrator_3` (`ecd27bad-3488-46aa-ba26-8b0b7cecf0eb`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/reviewer_p3_audio_1`  
**Milestone**: Phase 3 Milestone 2 (Pure-Rust Audio Output & Resampling Engine)  

---

## 1. Observation

1. **Workspace Compilation & Audits**:
   - `./scripts/audit_dependencies.sh`: Clean, zero banned dependencies.
   - `cargo check --workspace --all-targets`: Passed with 0 compiler warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings`: Passed with 0 clippy warnings.
   - `python3 scripts/intent_harness.py`: Passed 8/8 intents (100.0% compliance).

2. **Source Code Inspection of `crates/syncplay-codecs/src/audio/resampler.rs`**:
   - Line 138:
     ```rust
     let speed_ratio = state.speed_ratio;
     let step = (self.in_rate as f64) / ((self.out_rate as f64) * speed_ratio);
     ```
   - Line 326–333:
     ```rust
     // At 1.04x speed, step is smaller (48000 / (48000 * 1.04) = 1/1.04 = ~0.9615)
     // so more output frames are generated
     let out_frames = output.len() / 2;
     let expected_frames = (4800.0f64 * 1.04).round() as usize;
     assert!(
         (out_frames as isize - expected_frames as isize).abs() <= 2,
         "Expected ~{} frames, got {}",
         expected_frames,
         out_frames
     );
     ```

3. **Source Code Inspection of `crates/syncplay-codecs/tests/audio_resampler_tests.rs`**:
   - Line 125–130:
     ```rust
     let out = resampler.resample(&input).unwrap();
     let expected_frames = (4800.0 * ratio).round() as usize;
     let actual_frames = out.len() / 2;
     ```

4. **Multi-Channel Coverage**:
   - File doc comment in `tests/audio_resampler_tests.rs:8` advertises `//! - Multi-channel interleaving (mono, stereo, 6-channel).`, but no 6-channel test is present in the file.

---

## 2. Logic Chain

1. **Dimensional Analysis of Resampler Output vs. Physical Playback**:
   - In `resample_into`, `t` tracks fractional position in the input buffer. For each output frame produced, `t` advances by `step`.
   - The output samples are streamed to a Linux audio hardware DAC (`pw-cat`/`aplay`) which runs at a fixed physical clock rate $f_{\text{out}}$ (48,000 samples/sec).
   - In 1 second of physical playback, the DAC consumes $f_{\text{out}}$ output frames.
   - The number of input audio frames consumed in that 1 physical second is:
     $$\text{Input frames consumed per second} = f_{\text{out}} \times \text{step}$$
   - Substituting line 138:
     $$\text{Input frames consumed per second} = f_{\text{out}} \times \left( \frac{f_{\text{in}}}{f_{\text{out}} \times r} \right) = \frac{f_{\text{in}}}{r}$$
   - When the network PLL commands speed-up ($r = 1.04$), input consumption is $\frac{f_{\text{in}}}{1.04} \approx 0.9615 \times f_{\text{in}}$ frames per second. Input audio is consumed **4% slower**.
   - When the network PLL commands slow-down ($r = 0.96$), input consumption is $\frac{f_{\text{in}}}{0.96} \approx 1.0417 \times f_{\text{in}}$ frames per second. Input audio is consumed **4.17% faster**.
   - This proves that `step = in_rate / (out_rate * r)` inverts the speed steering multiplier ($r \mapsto 1/r$).

2. **Systemic PLL Instability**:
   - When local media lags behind the network clock reference ($e = \tau_{\text{ref}} - \tau_{\text{local}} > 0$), the PLL increases $r > 1.0$ to catch up.
   - Because the resampler inverts $r$, playback slows down, increasing lag $e$.
   - This converts negative feedback into positive feedback, causing phase error to diverge into Zone 3 hard seeks ($|e| > 1500\text{ms}$).

3. **Self-Certifying Tests**:
   - Tests in `resampler.rs` and `audio_resampler_tests.rs` asserted `out_frames = in_frames * ratio`.
   - The tests verified that the code matched the flawed formula $\text{step} = f_{\text{in}} / (f_{\text{out}} \times r)$ rather than the physical reality of DAC audio playback.

4. **Correct Formulation**:
   - To consume $r \times f_{\text{in}}$ input frames per physical second, we require:
     $$f_{\text{out}} \times \text{step} = f_{\text{in}} \times r \implies \text{step} = \frac{f_{\text{in}} \times r}{f_{\text{out}}}$$
   - And the number of output frames produced from $N_{\text{in}}$ input frames is:
     $$N_{\text{out}} = \frac{N_{\text{in}}}{\text{step}} = N_{\text{in}} \times \frac{f_{\text{out}}}{f_{\text{in}} \times r}$$
     For $f_{\text{in}} = f_{\text{out}}$, $N_{\text{out}} = N_{\text{in}} / r$.

---

## 3. Caveats

- For standard rate conversion at nominal speed ($r = 1.0$, such as 44.1 kHz $\leftrightarrow$ 48.0 kHz), the formula evaluates to $f_{\text{in}} / f_{\text{out}}$, which is completely correct. The bug only activates when micro-speed steering is engaged ($r \neq 1.0$).
- Phase tracking across arbitrary chunk boundaries (including prime and 1-sample splits) was independently tested and verified to be mathematically exact (sample error $< 3.2 \times 10^{-12}$).
- Hot loop zero-allocation capacity retention was independently verified.

---

## 4. Conclusion

**Verdict**: **REQUEST_CHANGES**

The pull request cannot be approved until the micro-speed steering formula is inverted to the correct form:
1. Update `crates/syncplay-codecs/src/audio/resampler.rs:138` to:
   ```rust
   let step = ((self.in_rate as f64) * speed_ratio) / (self.out_rate as f64);
   ```
2. Update unit test `test_speed_ratio_micro_steering` in `resampler.rs:329` to expect:
   ```rust
   let expected_frames = (4800.0f64 / 1.04).round() as usize;
   ```
3. Update integration test in `crates/syncplay-codecs/tests/audio_resampler_tests.rs:126` to expect:
   ```rust
   let expected_frames = (4800.0 / ratio).round() as usize;
   ```
4. Add an explicit 6-channel (5.1 surround) integration test to `tests/audio_resampler_tests.rs` to satisfy the advertised multi-channel contract.

---

## 5. Verification Method

To independently verify the bug and the proposed fix:

```bash
# 1. Run python dimensional proof test
python3 -c '
in_rate = 48000
out_rate = 48000
speed_ratio = 1.04 # 4% faster

# Current code:
step_code = in_rate / (out_rate * speed_ratio) # 0.9615
# Correct code:
step_correct = (in_rate * speed_ratio) / out_rate # 1.04

in_samples = 48000 # 1 second of audio
out_samples_code = in_samples / step_code # 49920 samples -> takes 49920/48000 = 1.04s to play (SLOWER)
out_samples_correct = in_samples / step_correct # 46154 samples -> takes 46154/48000 = 0.9615s to play (FASTER)

print(f"Current code duration: {out_samples_code / 48000:.4f}s (Speed: {1.0 / (out_samples_code / 48000):.4f}x)")
print(f"Correct code duration: {out_samples_correct / 48000:.4f}s (Speed: {1.0 / (out_samples_correct / 48000):.4f}x)")
'

# 2. Run workspace check, clippy, and tests
export PATH="$HOME/.cargo/bin:$PATH"
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
