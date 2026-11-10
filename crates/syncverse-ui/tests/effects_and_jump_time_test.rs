//! Verification and adversarial test suite for 10-Band Equalizer, Video Picture Adjustments,
//! Jump to Specific Time dialog, and extended player commands.

use syncverse_codecs::audio::{
    AudioEqualizer10Band, AudioEqualizerConfig, EqualizerPreset,
};
use syncverse_ui::command::PlayerCommand;
use syncverse_ui::gui::effects_dialog::{EffectsDialog, EffectsTab};
use syncverse_ui::gui::jump_time::{parse_time_input, JumpToTimeDialog};
use syncverse_ui::gui::video_effects::VideoColorAdjustments;
use syncverse_ui::gui::viewport::VideoTextureBuffer;
use syncverse_codecs::VideoFrameRGBA;

#[test]
fn test_audio_equalizer_full_presets_and_frequency_response() {
    for preset in EqualizerPreset::ALL {
        let name = preset.name();
        assert!(!name.is_empty(), "Preset name must not be empty");

        let cfg = AudioEqualizerConfig::from_preset(*preset);
        assert!(cfg.enabled, "Loaded preset must be enabled");
        assert_eq!(cfg.bands, preset.band_gains());

        let mut eq = AudioEqualizer10Band::new(48000, cfg);
        if *preset == EqualizerPreset::Flat {
            assert!(!eq.is_active(), "Flat preset with 0 dB pre-amp must be inactive (bypass)");
        } else {
            assert!(eq.is_active(), "Non-flat preset must be active");
        }

        // Test with arbitrary audio data
        let mut audio_buffer = vec![0.1f32; 960];
        eq.process_interleaved(&mut audio_buffer);

        for &sample in &audio_buffer {
            assert!(sample.is_finite(), "Output sample must be finite");
            assert!(sample.abs() <= 1.0, "Output must be bounded within [-1.0, 1.0]");
        }
    }
}

#[test]
fn test_audio_equalizer_preamp_and_clipping_protection() {
    let mut cfg = AudioEqualizerConfig::flat();
    cfg.enabled = true;
    cfg.pre_amp_db = 12.0; // Significant pre-amp boost (~4x)
    let mut eq = AudioEqualizer10Band::new(48000, cfg);

    // Audio samples near full scale
    let mut loud_samples = vec![0.8f32; 480];
    eq.process_interleaved(&mut loud_samples);

    for &s in &loud_samples {
        assert!(s.is_finite());
        // Soft clipping must keep samples smoothly below 1.0 without wrap-around distortion
        assert!((0.85..=1.0).contains(&s), "Loud boosted sample must be softly saturated in [0.85, 1.0]");
    }
}

#[test]
fn test_video_adjustments_lut_and_color_grading() {
    let mut adj = VideoColorAdjustments::default();
    assert!(adj.is_identity());

    // Neutral identity LUT must be 0..=255
    let lut = adj.compute_lut();
    for (i, &v) in lut.iter().enumerate() {
        assert_eq!(i as u8, v, "Default LUT must be exact identity");
    }

    // High contrast test: low values get darker, high values get brighter
    adj.enabled = true;
    adj.contrast = 1.5;
    let lut_contrast = adj.compute_lut();
    assert!(lut_contrast[64] < 64, "Low values should be crushed in high contrast");
    assert!(lut_contrast[192] > 192, "High values should be pushed higher in high contrast");

    // Saturation test: RGB [200, 100, 50, 255]
    let mut src_frame = VideoFrameRGBA::new(2, 2, 1000);
    {
        let slice = src_frame.data.as_mut_slice();
        for y in 0..2 {
            let row = y * src_frame.stride;
            for x in 0..2 {
                let px = row + x * 4;
                slice[px] = 200;
                slice[px + 1] = 100;
                slice[px + 2] = 50;
                slice[px + 3] = 255;
            }
        }
    }

    let mut buf = VideoTextureBuffer::new();

    // 1. Desaturation (Grayscale)
    let adj_gray = VideoColorAdjustments {
        enabled: true,
        saturation: 0.0,
        ..Default::default()
    };
    let gray_out = buf.prepare_with_adjustments(&src_frame, &adj_gray).to_vec();
    // In grayscale, R, G, B must be equal
    assert_eq!(gray_out[0], gray_out[1]);
    assert_eq!(gray_out[1], gray_out[2]);
    assert_eq!(gray_out[3], 255);

    // 2. Over-saturation (Vibrant)
    let adj_vivid = VideoColorAdjustments {
        enabled: true,
        saturation: 2.0,
        ..Default::default()
    };
    let vivid_out = buf.prepare_with_adjustments(&src_frame, &adj_vivid).to_vec();
    // Red channel should increase relative to green/blue
    assert!(vivid_out[0] >= gray_out[0]);
    assert!(vivid_out[2] <= gray_out[2]);
}

#[test]
fn test_jump_to_time_all_formats_and_limits() {
    let dur = 3_600_000; // 1 hour = 3600s = 3,600,000 ms

    // Standard timecodes
    assert_eq!(parse_time_input("00:00:00", 0, dur), Ok(0));
    assert_eq!(parse_time_input("00:01:30", 0, dur), Ok(90_000));
    assert_eq!(parse_time_input("01:00:00", 0, dur), Ok(3_600_000));
    assert_eq!(parse_time_input("45:15", 0, dur), Ok(2_715_000));

    // Pure seconds and units
    assert_eq!(parse_time_input("120", 0, dur), Ok(120_000));
    assert_eq!(parse_time_input("30s", 0, dur), Ok(30_000));
    assert_eq!(parse_time_input("5m", 0, dur), Ok(300_000));

    // Relative offsets
    assert_eq!(parse_time_input("+10s", 50_000, dur), Ok(60_000));
    assert_eq!(parse_time_input("-20s", 50_000, dur), Ok(30_000));
    assert_eq!(parse_time_input("+1m", 50_000, dur), Ok(110_000));
    assert_eq!(parse_time_input("-2m", 50_000, dur), Ok(0)); // Clamped at 0

    // Duration limit clamping
    assert_eq!(parse_time_input("02:00:00", 0, dur), Ok(dur));
    assert_eq!(parse_time_input("+5000s", 0, dur), Ok(dur));

    // Adversarial / Malformed inputs
    assert!(parse_time_input("not-a-number", 0, dur).is_err());
    assert!(parse_time_input("99:99:99:99", 0, dur).is_err());
    assert!(parse_time_input("   ", 0, dur).is_err());
}

#[test]
fn test_jump_to_time_dialog_lifecycle() {
    let mut dlg = JumpToTimeDialog::new();
    assert!(!dlg.is_open);

    dlg.open(125_000);
    assert!(dlg.is_open);
    assert!(dlg.input_text.contains("02:05"));

    dlg.close();
    assert!(!dlg.is_open);
}

#[test]
fn test_effects_dialog_state_and_actions() {
    let mut dlg = EffectsDialog::new();
    assert!(!dlg.is_open);
    assert_eq!(dlg.active_tab, EffectsTab::Audio);

    dlg.open(EffectsTab::Video);
    assert!(dlg.is_open);
    assert_eq!(dlg.active_tab, EffectsTab::Video);

    dlg.selected_preset = EqualizerPreset::Vocal;
    dlg.equalizer_config = AudioEqualizerConfig::from_preset(EqualizerPreset::Vocal);
    assert!(dlg.equalizer_config.enabled);
    assert_eq!(dlg.equalizer_config.bands[4], 5.0); // 1kHz boosted in Vocal preset
}

#[test]
fn test_player_commands_equalizer_and_dynamics() {
    let cfg = AudioEqualizerConfig::from_preset(EqualizerPreset::Rock);
    let cmd_eq = PlayerCommand::SetAudioEqualizer(cfg);
    let cmd_comp = PlayerCommand::SetAudioCompressor {
        enabled: true,
        threshold_db: -18.0,
        ratio: 4.0,
        makeup_gain_db: 3.0,
    };
    let cmd_pan = PlayerCommand::SetAudioStereoPan(-0.75);

    assert_eq!(cmd_eq, PlayerCommand::SetAudioEqualizer(cfg));
    assert_ne!(cmd_eq, cmd_comp);
    assert_eq!(cmd_pan, PlayerCommand::SetAudioStereoPan(-0.75));
}
