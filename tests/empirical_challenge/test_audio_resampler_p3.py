#!/usr/bin/env python3
"""
Empirical Challenge: Milestone 2 Audio Resampler Suite (Phase 3 Requirement R2)
Verifies:
1. Pure-Rust LinearResampler mathematical properties and phase accumulation.
2. AudioPacer timing decisions, underrun silence injection, and ramp-in smoothing.
3. AudioSink f32-LE wire serialization.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import test_audio_resampler_empirical
import test_audio_resampler_pacer_p3


def main():
    print("==============================================================================")
    print(" SYNCPLAY-ALT: PHASE 3 MILESTONE 2 AUDIO RESAMPLER EMPIRICAL SUITE")
    print("==============================================================================")
    print("--- [1/2] Running Audio LinearResampler Empirical Verification ---")
    ret1 = test_audio_resampler_empirical.main()
    assert ret1 == 0 or ret1 is None, f"test_audio_resampler_empirical returned {ret1}"

    print("\n--- [2/2] Running Audio Resampler & Pacer P3 Harness ---")
    ret2 = test_audio_resampler_pacer_p3.main()
    assert ret2 == 0 or ret2 is None, f"test_audio_resampler_pacer_p3 returned {ret2}"

    print("\n[SUCCESS] ALL AUDIO RESAMPLER EMPIRICAL CHALLENGES PASSED (100% VERIFIED).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
