/*
 * Copyright (C) 2026 Marinus Burger
 */

//! Plugin parameter definitions.
//!
//! All nih-plug `Params` structs live here, together with the `NamModel` enum
//! that is both a plugin parameter and referenced by the preset system.

use nih_plug::prelude::*;
use nih_plug_vizia::ViziaState;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::filter::{FilterPosition, FilterType};

// ── Embedded NAM model content ─────────────────────────────────────────────────

pub const NAM_MODEL_PHILIPS: &str =
    include_str!("resource/nam/Philips_EL3541D.nam");
pub const NAM_MODEL_CULTURE_VULTURE: &str =
    include_str!("resource/nam/Culture_Vulture_HIGH_OD_Bias 3_Drive_14_PK5.nam");
pub const NAM_MODEL_JH24: &str =
    include_str!("resource/nam/TAPE_OUT_JH24_30-450_L.nam");

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// Maps a normalised 0–1 value to a logarithmic frequency range [min, max].
pub fn log_scale(value: f32, min: f32, max: f32) -> f32 {
    min * (max / min).powf(value)
}

// ── NAM model selector ──────────────────────────────────────────────────────────

#[derive(PartialEq, Eq, Clone, Copy, Enum, Serialize, Deserialize, Debug)]
pub enum NamModel {
    #[name = "Philips EL3541D"]
    PhilipsEL3541D,
    #[name = "CultureVulture"]
    CultureVulture,
    #[name = "JH24"]
    JH24,
}

// ── KickParams ──────────────────────────────────────────────────────────────────

#[derive(Params)]
pub struct KickParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<ViziaState>,

    /// The main "Tune" of the kick (Hz)
    #[id = "tune"]
    pub tune: FloatParam,

    #[id = "waveform"]
    pub waveform: IntParam,

    /// How much the pitch sweeps down (Hz)
    #[id = "sweep"]
    pub sweep: FloatParam,

    /// Pitch decay time (ms)
    #[id = "pitch_decay"]
    pub pitch_decay: FloatParam,

    /// Drive / Distortion amount
    #[id = "drive"]
    pub drive: FloatParam,

    #[id = "drive_model"]
    pub drive_model: IntParam,

    /// Texture Amount
    #[id = "tex_amt"]
    pub tex_amt: FloatParam,

    /// Texture Decay time (ms)
    #[id = "tex_decay"]
    pub tex_decay: FloatParam,

    /// Texture Variation (0.0 = static, 1.0 = completely random)
    #[id = "tex_variation"]
    pub tex_variation: FloatParam,

    /// Analog Pitch Drift Variation (0.0 = stable, 1.0 = constantly drifting)
    #[id = "analog_variation"]
    pub analog_variation: FloatParam,

    /// Texture Type (1: Dust, 2: Crackle, 3: Sampled Noise, 4: Organic WT, 5: Vinyl Hiss/Pop, 6: Electrical Zap)
    #[id = "tex_type"]
    pub tex_type: IntParam,

    /// Texture Tone / Frequency / Density
    #[id = "tex_tone"]
    pub tex_tone: FloatParam,

    /// Amplitude Attack time (ms)
    #[id = "attack"]
    pub attack: FloatParam,

    /// Amplitude Decay time (ms)
    #[id = "decay"]
    pub decay: FloatParam,

    /// Amplitude Sustain level (0.0 - 1.0)
    #[id = "sustain"]
    pub sustain: FloatParam,

    /// Amplitude Release time (ms)
    #[id = "release"]
    pub release: FloatParam,

    #[id = "corrosion_frequency"]
    pub corrosion_frequency: FloatParam,

    #[id = "corrosion_width"]
    pub corrosion_width: FloatParam,

    #[id = "corrosion_noise_blend"]
    pub corrosion_noise_blend: FloatParam,

    #[id = "corrosion_stereo"]
    pub corrosion_stereo: FloatParam,

    #[id = "corrosion_amount"]
    pub corrosion_amount: FloatParam,

    /// Manual Trigger Button
    #[id = "trigger"]
    pub trigger: BoolParam,

    /// Trigger Logic for the UI (consumed by audio thread)
    pub gui_trigger: std::sync::atomic::AtomicU32,
    /// Release Logic for the UI (consumed by audio thread)
    pub gui_release: std::sync::atomic::AtomicU32,

    #[id = "bass_synth_mode"]
    pub bass_synth_mode: BoolParam,

    #[id = "nam_active"]
    pub nam_active: BoolParam,

    #[id = "nam_input_gain"]
    pub nam_input_gain: FloatParam,

    #[id = "output_gain"]
    pub output_gain: FloatParam,

    #[id = "nam_model"]
    pub nam_model: EnumParam<NamModel>,

    pub nam_is_loaded: AtomicBool,
    pub nam_status_text: Arc<RwLock<String>>,

    // ── Filter Engine ─────────────────────────────────────────────────────────
    #[id = "filter_active"]
    pub filter_active: BoolParam,

    /// LP24 / LP12 / HP24 / HP12 / BP12 / Notch
    #[id = "filter_type"]
    pub filter_type: EnumParam<FilterType>,

    /// Where in the signal chain the filter is inserted.
    #[id = "filter_position"]
    pub filter_position: EnumParam<FilterPosition>,

    /// Base cutoff frequency in Hz (log-skewed, 20–20 000 Hz).
    #[id = "filter_cutoff"]
    pub filter_cutoff: FloatParam,

    /// Resonance / Q (0.0 = clean, 1.0 = self-oscillation for LP24/HP24).
    #[id = "filter_resonance"]
    pub filter_resonance: FloatParam,

    /// How many octaves the ADSR envelope moves the cutoff.
    /// Positive = opens on attack; negative = closes on attack.
    #[id = "filter_env_amount"]
    pub filter_env_amount: FloatParam,

    #[id = "filter_env_attack"]
    pub filter_env_attack: FloatParam,

    #[id = "filter_env_decay"]
    pub filter_env_decay: FloatParam,

    #[id = "filter_env_sustain"]
    pub filter_env_sustain: FloatParam,

    #[id = "filter_env_release"]
    pub filter_env_release: FloatParam,

    /// When true (default): the filter envelope fires and completes its full
    /// A→D→R cycle on its own, independent of note length — like Kick 2/3 or
    /// the Roland 808.  When false: classic gate behaviour (sustain held until
    /// note-off, then release).
    #[id = "filter_env_trigger"]
    pub filter_env_trigger: BoolParam,

    /// Pre-filter gain (0 = clean, higher = more harmonic saturation).
    #[id = "filter_drive"]
    pub filter_drive: FloatParam,

    /// How much the MIDI note frequency shifts the cutoff (0 = off, 1 = full).
    #[id = "filter_key_track"]
    pub filter_key_track: FloatParam,
}

impl KickParams {
    pub fn get_current_preset(&self) -> crate::presets::Preset {
        crate::presets::Preset {
            name: "Custom".to_string(),
            tune: self.tune.value(),
            waveform: self.waveform.value(),
            sweep: self.sweep.value(),
            pitch_decay: self.pitch_decay.value(),
            drive: self.drive.value(),
            drive_model: self.drive_model.value(),
            tex_amt: self.tex_amt.value(),
            tex_decay: self.tex_decay.value(),
            tex_variation: self.tex_variation.value(),
            analog_variation: self.analog_variation.value(),
            tex_type: self.tex_type.value(),
            tex_tone: self.tex_tone.value(),
            attack: self.attack.value(),
            decay: self.decay.value(),
            sustain: self.sustain.value(),
            release: self.release.value(),
            corrosion_frequency: self.corrosion_frequency.value(),
            corrosion_width: self.corrosion_width.value(),
            corrosion_noise_blend: self.corrosion_noise_blend.value(),
            corrosion_stereo: self.corrosion_stereo.value(),
            corrosion_amount: self.corrosion_amount.value(),
            bass_synth_mode: self.bass_synth_mode.value(),
            nam_active: self.nam_active.value(),
            nam_input_gain: self.nam_input_gain.value(),
            output_gain: self.output_gain.value(),
            nam_model: self.nam_model.value(),
            filter_active: self.filter_active.value(),
            filter_type: self.filter_type.value(),
            filter_position: self.filter_position.value(),
            filter_cutoff: self.filter_cutoff.value(),
            filter_resonance: self.filter_resonance.value(),
            filter_env_amount: self.filter_env_amount.value(),
            filter_env_attack: self.filter_env_attack.value(),
            filter_env_decay: self.filter_env_decay.value(),
            filter_env_sustain: self.filter_env_sustain.value(),
            filter_env_release: self.filter_env_release.value(),
            filter_env_trigger: self.filter_env_trigger.value(),
            filter_drive: self.filter_drive.value(),
            filter_key_track: self.filter_key_track.value(),
        }
    }
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            editor_state: ViziaState::new_with_default_scale_factor(|| (1400, 1100), 1.0),

            gui_trigger: std::sync::atomic::AtomicU32::new(0),
            gui_release: std::sync::atomic::AtomicU32::new(0),

            tune: FloatParam::new(
                "Tune",
                44.0,
                FloatRange::Linear {
                    min: 30.0,
                    max: 150.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} hz", value))),

            waveform: IntParam::new(
                "Mode",
                1i32,
                IntRange::Linear {
                    min: 1i32,
                    max: 5i32,
                },
            )
            .with_value_to_string(Arc::new(|value| match value {
                1 => "Sine".to_string(),
                2 => "Octave".to_string(),
                3 => "Fifth".to_string(),
                4 => "Warm".to_string(),
                5 => "Sub".to_string(),
                _ => "Unknown".to_string(),
            })),

            sweep: FloatParam::new(
                "Sweep",
                239.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} hz", value))),

            pitch_decay: FloatParam::new(
                "Decay",
                100.0,
                FloatRange::Linear {
                    min: 5.0,
                    max: 500.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} ms", value))),

            drive: FloatParam::new("Gain", 0.2, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            drive_model: IntParam::new(
                "Mode",
                1i32,
                IntRange::Linear {
                    min: 1i32,
                    max: 5i32,
                },
            )
            .with_value_to_string(Arc::new(|value| match value {
                1 => "Tape Classic".to_string(),
                2 => "Tape Modern".to_string(),
                3 => "Tube Triode".to_string(),
                4 => "Tube Pentode".to_string(),
                5 => "Digital".to_string(),
                _ => "Unknown".to_string(),
            })),

            tex_amt: FloatParam::new("Amount", 0.4, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            tex_decay: FloatParam::new(
                "Decay",
                80.0,
                FloatRange::Linear {
                    min: 5.0,
                    max: 650.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} ms", value))),

            tex_variation: FloatParam::new(
                "Variation",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            analog_variation: FloatParam::new(
                "Instability",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            tex_type: IntParam::new(
                "Type",
                5i32,
                IntRange::Linear {
                    min: 1i32,
                    max: 6i32,
                },
            )
            .with_value_to_string(Arc::new(|value| match value {
                1 => "Dust".to_string(),
                2 => "Crackle".to_string(),
                3 => "Sampled".to_string(),
                4 => "Organic".to_string(),
                5 => "Vinyl".to_string(),
                6 => "Zap".to_string(),
                _ => "Unknown".to_string(),
            })),

            tex_tone: FloatParam::new("Tone", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            attack: FloatParam::new(
                "[A]",
                0.1,
                FloatRange::Linear {
                    min: 0.1,
                    max: 1000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} ms", value))),

            decay: FloatParam::new(
                "[D]",
                153.0,
                FloatRange::Linear {
                    min: 10.0,
                    max: 1000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} ms", value))),

            sustain: FloatParam::new("[S]", 0.44, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            release: FloatParam::new(
                "[R]",
                128.0,
                FloatRange::Linear {
                    min: 10.0,
                    max: 2000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} ms", value))),

            corrosion_frequency: FloatParam::new(
                "Freq",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(move |value| {
                format!("{:.0} hz", log_scale(value, 15.0, 22000.0))
            })),

            corrosion_width: FloatParam::new(
                "Width",
                0.5,
                FloatRange::Linear { min: 0.1, max: 2.5 },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1}", value))),

            corrosion_noise_blend: FloatParam::new(
                "Sine ~ Noise",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            corrosion_stereo: FloatParam::new(
                "Stereo",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            corrosion_amount: FloatParam::new(
                "Amount",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            trigger: BoolParam::new("Trigger", false),

            bass_synth_mode: BoolParam::new("808 Mode", false),

            nam_active: BoolParam::new("NAM Active", false),

            nam_input_gain: FloatParam::new(
                "NAM Input Gain",
                0.0,
                FloatRange::Linear {
                    min: -18.0,
                    max: 18.0,
                },
            )
            .with_unit("dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            output_gain: FloatParam::new(
                "Main Out",
                0.0,
                FloatRange::Linear {
                    min: -18.0,
                    max: 18.0,
                },
            )
            .with_unit("dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            nam_model: EnumParam::new("Model", NamModel::PhilipsEL3541D),
            nam_is_loaded: AtomicBool::new(false),
            nam_status_text: Arc::new(RwLock::new(String::from("No model loaded"))),

            // ── Filter Engine ─────────────────────────────────────────────────
            filter_active: BoolParam::new("Filter", false),

            filter_type: EnumParam::new("Filter Type", FilterType::LP24),

            filter_position: EnumParam::new("Filter Position", FilterPosition::PostNam),

            filter_cutoff: FloatParam::new(
                "Filter Cutoff",
                1500.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
            .with_smoother(SmoothingStyle::Logarithmic(30.0)),

            filter_resonance: FloatParam::new(
                "Resonance",
                0.16,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(20.0)),

            filter_env_amount: FloatParam::new(
                "Env Amount",
                4.0,
                // ±4 octaves covers every practical kick drum sweep (e.g. 300 Hz → 4.8 kHz).
                // The previous ±10 oct range allowed the filter to be slammed against the
                // Nyquist ceiling, producing a loud high-pitched screech at moderate resonance.
                FloatRange::Linear { min: -4.0, max: 4.0 },
            )
            .with_unit(" oct")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            filter_env_attack: FloatParam::new(
                "Flt Attack",
                0.1,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 500.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            filter_env_decay: FloatParam::new(
                "Flt Decay",
                230.0,
                FloatRange::Skewed {
                    min: 5.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            filter_env_sustain: FloatParam::new(
                "Flt Sustain",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            filter_env_release: FloatParam::new(
                "Flt Release",
                200.0,
                FloatRange::Skewed {
                    min: 5.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            // Trigger mode ON by default: the filter envelope always completes
            // its A→D→R cycle on its own, independent of note length.
            // Set to false for classic gate/synth behaviour.
            filter_env_trigger: BoolParam::new("Filter Trigger", true)
                .with_value_to_string(Arc::new(|v| {
                    if v { "Trigger".to_string() } else { "Gate".to_string() }
                }))
                .with_string_to_value(Arc::new(|s| {
                    match s.trim().to_lowercase().as_str() {
                        "trigger" | "on" | "true" => Some(true),
                        "gate" | "off" | "false" => Some(false),
                        _ => None,
                    }
                })),

            filter_drive: FloatParam::new(
                "Filter Drive",
                0.0,
                FloatRange::Linear { min: 0.0, max: 5.0 },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            filter_key_track: FloatParam::new(
                "Key Track",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),
        }
    }
}
