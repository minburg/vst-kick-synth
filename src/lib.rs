/*
 * Copyright (C) 2026 Marinus Burger
 */

use nih_plug::prelude::AtomicF32;
use nih_plug::prelude::*;
use nih_plug_vizia::ViziaState;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod editor;
mod presets;
#[cfg(feature = "nam")]
mod nam;
mod filter;
pub use filter::{FilterType, FilterPosition};
use filter::FilterEngine;

const NAM_MODEL_PHILIPS: &str = include_str!("resource/nam/Philips_EL3541D.nam");
const NAM_MODEL_CULTURE_VULTURE: &str =
    include_str!("resource/nam/Culture_Vulture_HIGH_OD_Bias 3_Drive_14_PK5.nam");
const NAM_MODEL_JH24: &str = include_str!("resource/nam/TAPE_OUT_JH24_30-450_L.nam");

// Helper function to map a 0-1 value to a logarithmic frequency range
fn log_scale(value: f32, min: f32, max: f32) -> f32 {
    min * (max / min).powf(value)
}

#[derive(Clone)]
struct VoiceState {
    phase: f32,
    envelope_value: f32,
    current_phase: EnvelopePhase,
    phase_timer: f32,
    release_coeff: f32,
    pitch_env_timer: f32,
    tex_env_value: f32,
    tex_env_phase: EnvelopePhase,
    wt_phase: f32,
    tex_filter_state: f32,
    tex_filter_state_2: f32,
    static_rng_state: u32,
    midi_velocity: f32,
    analog_drift: f32,
    fast_release: bool,
    midi_note: u8,
    /// Second oscillator phase used only in Sub mode (waveform 5).
    /// Advances at half the fundamental frequency so it never aliases when
    /// `voice.phase` wraps — it needs its own independent counter.
    waveform_phase2: f32,
}

impl Default for VoiceState {
    fn default() -> Self {
        Self {
            phase: 0.0,
            envelope_value: 0.0,
            current_phase: EnvelopePhase::Idle,
            phase_timer: 0.0,
            release_coeff: 0.0,
            pitch_env_timer: 0.0,
            tex_env_value: 0.0,
            tex_env_phase: EnvelopePhase::Idle,
            wt_phase: 0.0,
            tex_filter_state: 0.0,
            tex_filter_state_2: 0.0,
            static_rng_state: 1337,
            midi_velocity: 1.0,
            analog_drift: 0.0,
            fast_release: false,
            midi_note: 36,
            waveform_phase2: 0.0,
        }
    }
}

struct CorrosionState {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    sine_phase: f32,
    bp_l: [f32; 2],
    bp_r: [f32; 2],
    rng: u32,
}

pub struct KickSynth {
    params: Arc<KickParams>,
    sample_rate: f32,

    voice: VoiceState,
    releasing_voice: Option<VoiceState>,

    #[cfg(debug_assertions)]
    debug_phase: f32,
    #[cfg(debug_assertions)]
    debug_release_timer: f32,

    // Texture State
    wavetable: Vec<f32>,
    sampled_noise: Vec<f32>,
    free_rng_state: Cell<u32>,

    // Corrosion (Erosion-style phase-modulated delay) State
    corrosion: RefCell<CorrosionState>,

    meter_decay_per_sample: f32,
    peak_meter_l: Arc<AtomicF32>,
    peak_meter_r: Arc<AtomicF32>,

    // Trigger Logic
    was_trigger_on: bool,
    active_midi_note: Option<u8>,

    #[cfg(feature = "nam")]
    nam_synth: nam::NamSynth,
    current_nam_model: Option<NamModel>,
    /// Internal per-model output gain normalization (computed at load time, NOT user-controlled)
    nam_calibration_gain: f32,
    /// Internal per-model input pre-scale applied before the NAM block.
    /// Shifts the saturation threshold so all models feel comparable at the same user input gain.
    /// The output compensation for this shift is already folded into nam_calibration_gain.
    nam_pre_input_scale: f32,

    /// Filter engine: Moog Ladder + TPT SVF, stereo, with its own ADSR envelope.
    filter_engine: FilterEngine,
    /// Cached last filter position — used to detect changes and clear stale state.
    last_filter_position: FilterPosition,
    /// Cached last filter type — used to detect changes and clear stale state.
    last_filter_type: FilterType,

    mono_buffer: Vec<f32>,
    nam_output_buffer: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvelopePhase {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(PartialEq, Eq, Clone, Copy, Enum, Serialize, Deserialize, Debug)]
pub enum NamModel {
    #[name = "Philips EL3541D"]
    PhilipsEL3541D,
    #[name = "CultureVulture"]
    CultureVulture,
    #[name = "JH24"]
    JH24,
}

#[derive(Params)]
struct KickParams {
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
            editor_state: editor::default_state(),

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
                0.5, // Default to a middle value in the normalized range
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

impl Default for KickSynth {
    fn default() -> Self {
        let mut wavetable = vec![0.0; 2048];
        let mut sampled_noise = vec![0.0; (44100.0 * 0.5) as usize]; // 500ms
        let mut seed = 42u32;
        let mut next_rd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed as f32) / (u32::MAX as f32)
        };
        for i in 1..=32 {
            let harmonic = i as f32;
            let phase_offset = next_rd() * std::f32::consts::PI * 2.0;
            let amp = 1.0 / (harmonic.powf(1.5));
            for j in 0..2048 {
                let phase =
                    (j as f32 / 2048.0) * harmonic * std::f32::consts::PI * 2.0 + phase_offset;
                wavetable[j] += phase.sin() * amp;
            }
        }
        let max_val = wavetable.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if max_val > 0.0 {
            for sample in wavetable.iter_mut() {
                *sample /= max_val;
            }
        }

        // Generate fractional brownian motion sampled noise
        let mut value = 0.0f32;
        for i in 0..sampled_noise.len() {
            let white = next_rd() * 2.0 - 1.0;
            value += (white - value) * 0.1; // Pink-ish noise filter
            sampled_noise[i] = value;
        }
        let max_noise = sampled_noise.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if max_noise > 0.0 {
            for sample in sampled_noise.iter_mut() {
                *sample /= max_noise;
            }
        }

        // 2ms base delay + 1ms max mod depth at 44100 Hz = ~133 samples max
        // Allocate for up to 192kHz: ceil(192000 * 0.003) = 576, keep power-of-two margin
        let corrosion_buf_size = 2048usize;

        Self {
            params: Arc::new(KickParams::default()),
            sample_rate: 44100.0,
            voice: VoiceState::default(),
            releasing_voice: None,
            #[cfg(debug_assertions)]
            debug_phase: 0.0,
            #[cfg(debug_assertions)]
            debug_release_timer: -1.0,
            wavetable,
            sampled_noise,
            free_rng_state: Cell::new(80085),

            corrosion: RefCell::new(CorrosionState {
                buf_l: vec![0.0; corrosion_buf_size],
                buf_r: vec![0.0; corrosion_buf_size],
                write: 0,
                sine_phase: 0.0,
                bp_l: [0.0; 2],
                bp_r: [0.0; 2],
                rng: 0xDEAD_BEEF,
            }),

            meter_decay_per_sample: 1.0,
            peak_meter_l: Arc::new(AtomicF32::new(0.0)), // 0.0 Linear = Silence
            peak_meter_r: Arc::new(AtomicF32::new(0.0)),

            was_trigger_on: false,
            active_midi_note: None,

            #[cfg(feature = "nam")]
            nam_synth: nam::NamSynth::new(44100.0, 2048),
            current_nam_model: None,
            nam_calibration_gain: 1.0,
            nam_pre_input_scale: 1.0,

            filter_engine: FilterEngine::default(),
            last_filter_position: FilterPosition::PostNam,
            last_filter_type: FilterType::LP24,

            mono_buffer: Vec::with_capacity(2048),
            nam_output_buffer: Vec::with_capacity(2048),
        }
    }
}

struct SmoothedParams {
    tune: f32,
    waveform: i32,
    sweep: f32,
    pitch_decay: f32,
    drive: f32,
    drive_model: i32,
    tex_amt: f32,
    tex_decay: f32,
    tex_variation: f32,
    analog_variation: f32,
    tex_type: i32,
    tex_tone: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    corrosion_amount: f32,
    nam_input_gain: f32,
    output_gain: f32,
    bass_synth_mode: bool,
}

/// All NAM models are normalized to this integrated loudness (LUFS) after loading.
/// -18 LUFS gives headroom above broadcast (-23) while still being loud enough for a kick plugin.
#[cfg(feature = "nam")]
const NAM_TARGET_LUFS: f32 = -18.0;

/// Extracts `metadata.loudness` (LUFS) from a .nam JSON string.
/// Returns `None` if the field is absent or cannot be parsed.
/// Falls back to `NAM_TARGET_LUFS` so the calibration gain becomes 0 dB.
#[cfg(feature = "nam")]
fn parse_nam_loudness(content: &str) -> Option<f32> {
    let key = "\"loudness\":";
    let start = content.find(key)? + key.len();
    let slice = content[start..].trim_start();
    let end = slice
        .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
        .unwrap_or(slice.len());
    slice[..end].trim().parse::<f32>().ok()
}

/// Per-model fixed pre-gain (dB) applied to the signal *before* the NAM block.
///
/// This shifts each model's saturation onset to a comparable user input level.
/// Without correction, JH24 (a clean tape model) doesn't start saturating until
/// +15 dB user input, while the other models saturate around –15 dB — a 30 dB gap.
///
/// A +15 dB pre-boost on JH24 moves its saturation threshold to 0 dB user input,
/// which is a musically useful middle ground: clean headroom below 0, warm saturation
/// above, and heavy compression when driven hard — matching the feel of the other models.
///
/// The output impact of this pre-boost is compensated by subtracting the same dB value
/// from `calibration_db` at model load time, so the overall output level stays at –12 dBFS.
#[cfg(feature = "nam")]
fn model_pre_input_db(model: NamModel) -> f32 {
    match model {
        NamModel::PhilipsEL3541D => 0.0,   // saturates at –15 dB, no shift needed
        NamModel::CultureVulture => 0.0,   // saturates at –15 dB, no shift needed
        NamModel::JH24           => 15.0,  // saturates at +15 dB → shift to 0 dB
    }
}

/// Per-model empirical trim (dB) added on top of the loudness-based calibration.
///
/// WHY THIS EXISTS:
/// `metadata.loudness` normalises each model relative to its own *training* DI level.
/// Our synth's output amplitude at 0 dB input gain is unrelated to that training level,
/// so models with different saturation curves still land at different output levels.
/// Heavy saturators (Philips, Culture Vulture) are essentially self-limiting and stay
/// consistent across all input gains. Tape / more-linear models (JH24) track input
/// much more closely, so the loudness calibration alone over- or under-shoots.
///
/// These offsets are measured empirically: trigger the kick at 0 dB NAM input gain
/// and read the peak meter. Target is –12 dBFS.
///
/// HOW TO RE-MEASURE after changing the synth or adding a new model:
///   measured_db = peak meter reading at 0 dB input gain (with current calibration applied)
///   new_trim = current_trim + (–12.0 – measured_db)
///
/// Measured values that produced these trims (all at 0 dB input gain, target –12 dBFS):
///   PhilipsEL3541D : measured –12 dB  →  –5 dB trim  (no residual)
///   CultureVulture : measured –11 dB  →  –2 dB trim
///   JH24           : measured –15 dB  →  +3 dB trim  (model near saturation at 0 dB input
///                      due to +15 dB pre-input boost; operating regime shift is expected)
#[cfg(feature = "nam")]
fn model_reference_trim_db(model: NamModel) -> f32 {
    match model {
        NamModel::PhilipsEL3541D => -5.0,
        NamModel::CultureVulture => -2.0,
        NamModel::JH24           =>  3.0,
    }
}

impl Plugin for KickSynth {
    const NAME: &'static str = "Kick Synth";
    const VENDOR: &'static str = "Convolution DEV";
    const URL: &'static str = "https://github.com/minburg/vst-kick-synth";
    const EMAIL: &'static str = "email@example.com";
    const VERSION: &'static str = "0.2.1";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        _ctx: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = _buffer_config.sample_rate;

        // Resize corrosion delay buffers for the actual sample rate.
        let corrosion_buf_size =
            ((_buffer_config.sample_rate * 0.004) as usize + 4).next_power_of_two();
        let mut corrosion_state = self.corrosion.borrow_mut();
        corrosion_state.buf_l = vec![0.0; corrosion_buf_size];
        corrosion_state.buf_r = vec![0.0; corrosion_buf_size];
        corrosion_state.write = 0;

        let release_db_per_second = 160.0;

        // Calculate the constant for 1 sample of decay
        self.meter_decay_per_sample = f32::powf(
            10.0,
            -release_db_per_second / (20.0 * _buffer_config.sample_rate),
        );

        // Resize NAM buffers if needed (though max_buffer_size is usually stable)
        self.mono_buffer
            .resize(_buffer_config.max_buffer_size as usize, 0.0);
        self.nam_output_buffer
            .resize(_buffer_config.max_buffer_size as usize, 0.0);
        #[cfg(feature = "nam")]
        self.nam_synth.update_settings(
            _buffer_config.sample_rate,
            _buffer_config.max_buffer_size as i32,
        );

        true
    }

    fn reset(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.debug_phase = 0.0;
            self.debug_release_timer = -1.0;
        }
        self.voice = VoiceState::default();
        self.releasing_voice = None;
        self.was_trigger_on = false;
        self.active_midi_note = None;

        // Clear corrosion state
        let mut corrosion_state = self.corrosion.borrow_mut();
        corrosion_state.buf_l.iter_mut().for_each(|s| *s = 0.0);
        corrosion_state.buf_r.iter_mut().for_each(|s| *s = 0.0);
        corrosion_state.write = 0;
        corrosion_state.sine_phase = 0.0;
        corrosion_state.bp_l = [0.0; 2];
        corrosion_state.bp_r = [0.0; 2];
        self.filter_engine.clear();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _ctx: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut next_event = _ctx.next_event();

        let mut max_amplitude_in_block_l: f32 = 0.0;
        let mut max_amplitude_in_block_r: f32 = 0.0;

        // 0. Update NAM model if changed
        #[cfg(feature = "nam")]
        {
            let selected_model = self.params.nam_model.value();
            if self.current_nam_model != Some(selected_model) {
                let _content = match selected_model {
                    NamModel::PhilipsEL3541D => NAM_MODEL_PHILIPS,
                    NamModel::CultureVulture => NAM_MODEL_CULTURE_VULTURE,
                    NamModel::JH24 => NAM_MODEL_JH24,
                };

                match self.nam_synth.load_model_content(_content) {
                    Ok(_) => {
                        self.params.nam_is_loaded.store(true, Ordering::SeqCst);
                        if let Some(mut text) = self.params.nam_status_text.try_write() {
                            *text = String::from("NAM Loaded");
                        }
                        // Layer 1 — loudness calibration: normalises the model relative to its
                        // training DI level using the LUFS value baked into the .nam metadata.
                        let loudness = parse_nam_loudness(_content).unwrap_or(NAM_TARGET_LUFS);
                        let loudness_offset_db = NAM_TARGET_LUFS - loudness;

                        // Layer 2 — reference trim: corrects the residual mismatch between the
                        // training DI amplitude and our synth's output at 0 dB input gain.
                        // Target after both layers: –12 dBFS peak at 0 dB input gain.
                        let trim_db = model_reference_trim_db(selected_model);

                        // Layer 3 — pre-input shift: a fixed boost applied *before* the model
                        // that shifts the saturation onset to a comparable input level across
                        // all models. The same amount is subtracted here so the output level
                        // is unchanged in the linear range.
                        let pre_input_db = model_pre_input_db(selected_model);
                        self.nam_pre_input_scale = util::db_to_gain_fast(pre_input_db);

                        let calibration_db = loudness_offset_db + trim_db - pre_input_db;
                        self.nam_calibration_gain = util::db_to_gain_fast(calibration_db);
                        nih_log!(
                            "NAM calibration: loudness={:.2} LUFS → loudness offset={:.2} dB + trim={:.2} dB + pre-input={:.2} dB = {:.2} dB total",
                            loudness, loudness_offset_db, trim_db, pre_input_db, calibration_db
                        );
                        self.current_nam_model = Some(selected_model);
                    }
                    Err(e) => {
                        self.params.nam_is_loaded.store(false, Ordering::SeqCst);
                        if let Some(mut text) = self.params.nam_status_text.try_write() {
                            *text = format!("Error: {}", e);
                        }
                        nih_log!("Failed to load NAM model: {}", e);
                        self.current_nam_model = Some(selected_model);
                    }
                }
            }
        }

        // 1. Handle UI/Parameter Triggers (Block Rate)
        let trigger_count = self.params.gui_trigger.swap(0, Ordering::SeqCst);
        let release_count = self.params.gui_release.swap(0, Ordering::SeqCst);
        let trigger_param_val = self.params.trigger.value();

        // If we have UI triggers, process them
        if trigger_count > 0 {
            self.trigger_note(1.0, self.active_midi_note.unwrap_or(36).saturating_sub(24));
        }

        // Only release if we didn't just trigger in this exact block,
        // OR if we have more releases than triggers (meaning button was already down).
        if release_count > 0 && trigger_count == 0 {
            self.release_note();
        }

        // Fallback for automation
        if trigger_param_val && !self.was_trigger_on {
            self.trigger_note(1.0, self.active_midi_note.unwrap_or(36).saturating_sub(24));
        } else if !trigger_param_val && self.was_trigger_on {
            self.release_note();
        }
        self.was_trigger_on = trigger_param_val;

        // 2. Generate mono synth signal for the Block
        let num_samples = buffer.samples();

        // Filter: read block-rate params once; per-sample smoothed values (cutoff,
        // resonance) are consumed inside the individual filter processing loops below.
        let filter_active   = self.params.filter_active.value();
        let filter_type     = self.params.filter_type.value();
        let filter_position = self.params.filter_position.value();
        let filter_env_amount  = self.params.filter_env_amount.value();
        let filter_env_attack  = self.params.filter_env_attack.value();
        let filter_env_decay   = self.params.filter_env_decay.value();
        let filter_env_sustain = self.params.filter_env_sustain.value();
        let filter_env_release = self.params.filter_env_release.value();
        let filter_drive     = self.params.filter_drive.value();
        let filter_key_track = self.params.filter_key_track.value();
        let filter_midi_note = self.voice.midi_note;
        // Clear stale integrator state whenever the type or position changes.
        if filter_type != self.last_filter_type || filter_position != self.last_filter_position {
            self.filter_engine.clear();
            self.last_filter_type = filter_type;
            self.last_filter_position = filter_position;
        }

        #[cfg(feature = "nam")]
        let mut output_gains: Vec<f32> = Vec::with_capacity(num_samples);
        #[cfg(not(feature = "nam"))]
        let mut output_gains: Vec<f32> = Vec::with_capacity(num_samples);
        let nam_active_value = self.params.nam_active.value();

        for sample_idx in 0..num_samples {
            #[cfg(debug_assertions)]
            {
                self.debug_phase += 1.0 / self.sample_rate;
                if self.debug_phase >= 0.5 {
                    self.debug_phase -= 0.5;
                    self.trigger_note(0.8, 12); // C0
                    self.debug_release_timer = 0.2 * self.sample_rate;
                }

                if self.debug_release_timer > 0.0 {
                    self.debug_release_timer -= 1.0;
                    if self.debug_release_timer <= 0.0 {
                        self.release_note();
                    }
                }
            }

            // MIDI Handle
            while let Some(event) = next_event {
                if event.timing() > sample_idx as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { velocity, note, .. } => {
                        if velocity > 0.0 {
                            self.active_midi_note = Some(note);
                            self.trigger_note(velocity, note.saturating_sub(24));
                        } else {
                            if self.active_midi_note == Some(note) {
                                self.release_note();
                                self.active_midi_note = None;
                            }
                        }
                    }
                    NoteEvent::NoteOff { note, .. } => {
                        if self.active_midi_note == Some(note) {
                            self.release_note();
                            self.active_midi_note = None;
                        }
                    }
                    _ => (),
                }
                next_event = _ctx.next_event();
            }

            let params = SmoothedParams {
                tune: self.params.tune.smoothed.next(),
                waveform: self.params.waveform.smoothed.next(),
                sweep: self.params.sweep.smoothed.next(),
                pitch_decay: self.params.pitch_decay.smoothed.next(),
                drive: self.params.drive.smoothed.next(),
                drive_model: self.params.drive_model.value(),
                tex_amt: self.params.tex_amt.smoothed.next(),
                tex_decay: self.params.tex_decay.smoothed.next(),
                tex_variation: self.params.tex_variation.smoothed.next(),
                analog_variation: self.params.analog_variation.smoothed.next(),
                tex_type: self.params.tex_type.value(),
                tex_tone: self.params.tex_tone.smoothed.next(),
                attack: self.params.attack.smoothed.next(),
                decay: self.params.decay.smoothed.next(),
                sustain: self.params.sustain.smoothed.next(),
                release: self.params.release.smoothed.next(),
                corrosion_amount: self.params.corrosion_amount.smoothed.next(),
                nam_input_gain: self.params.nam_input_gain.smoothed.next(),
                output_gain: self.params.output_gain.smoothed.next(),
                bass_synth_mode: self.params.bass_synth_mode.value(),
            };

            // Process active voice
            let mut mono_sample = Self::compute_voice_sample(
                &mut self.voice,
                &params,
                self.sample_rate,
                &self.free_rng_state,
                &self.wavetable,
                &self.sampled_noise,
            );

            // Process releasing voice
            if let Some(releasing_voice) = &mut self.releasing_voice {
                mono_sample += Self::compute_voice_sample(
                    releasing_voice,
                    &params,
                    self.sample_rate,
                    &self.free_rng_state,
                    &self.wavetable,
                    &self.sampled_noise,
                );
                if releasing_voice.current_phase == EnvelopePhase::Idle
                    && releasing_voice.tex_env_phase == EnvelopePhase::Idle
                {
                    self.releasing_voice = None;
                }
            }

            let input_gain_amp = if cfg!(feature = "nam") && nam_active_value {
                // User-controlled drive × per-model pre-input shift (shifts saturation onset)
                util::db_to_gain_fast(params.nam_input_gain) * self.nam_pre_input_scale
            } else {
                1.0
            };
            self.mono_buffer[sample_idx] = mono_sample * input_gain_amp;

            // Collect smoothed master output gain for every sample, always (not just for NAM)
            output_gains.push(util::db_to_gain_fast(params.output_gain));
        }

        // Filter — PreNam position: applied after synthesis, before NAM saturation.
        // Shapes the tone going INTO the model — tighter, darker results.
        if filter_active && filter_position == FilterPosition::PreNam {
            for i in 0..num_samples {
                let cutoff    = self.params.filter_cutoff.smoothed.next();
                let resonance = self.params.filter_resonance.smoothed.next();
                self.mono_buffer[i] = self.filter_engine.process_mono(
                    self.mono_buffer[i], self.sample_rate, filter_type,
                    cutoff, resonance, filter_env_amount,
                    filter_env_attack, filter_env_decay, filter_env_sustain, filter_env_release,
                    filter_drive, filter_midi_note, filter_key_track,
                );
            }
        }

        // 3. Apply NAM Block if enabled and active
        #[cfg(feature = "nam")]
        if nam_active_value {
            self.nam_synth.process_block(
                &self.mono_buffer[0..num_samples],
                &mut self.nam_output_buffer[0..num_samples],
            );
        } else {
            // Bypass NAM: just copy input to output
            self.nam_output_buffer[0..num_samples]
                .copy_from_slice(&self.mono_buffer[0..num_samples]);
        }
        #[cfg(not(feature = "nam"))]
        self.nam_output_buffer[0..num_samples].copy_from_slice(&self.mono_buffer[0..num_samples]);

        // Filter — PostNam position: applied after NAM, before Corrosion.
        // Most common use: post-distortion roll-off / tonal shaping.
        if filter_active && filter_position == FilterPosition::PostNam {
            for i in 0..num_samples {
                let cutoff    = self.params.filter_cutoff.smoothed.next();
                let resonance = self.params.filter_resonance.smoothed.next();
                self.nam_output_buffer[i] = self.filter_engine.process_mono(
                    self.nam_output_buffer[i], self.sample_rate, filter_type,
                    cutoff, resonance, filter_env_amount,
                    filter_env_attack, filter_env_decay, filter_env_sustain, filter_env_release,
                    filter_drive, filter_midi_note, filter_key_track,
                );
            }
        }

        // 3. Post-NAM: Stereoize and write to output
        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {
            // NAM model calibration: internal, fixed per model, compensates for different model loudness
            let calibration_gain = if nam_active_value {
                #[cfg(feature = "nam")]
                { self.nam_calibration_gain }
                #[cfg(not(feature = "nam"))]
                { 1.0 }
            } else {
                1.0
            };

            // Master output trim: always applied, user-controlled
            let master_gain = output_gains[sample_idx];

            let driven = self.nam_output_buffer[sample_idx] * calibration_gain * master_gain;

            // Note: We use the current smoothed value of corrosion_amount for the second pass.
            // In a better implementation, we'd buffer the smoothed values too, but this is usually fine for parameters.
            let (mut out_l, mut out_r) = Self::apply_corrosion(
                self.sample_rate,
                driven,
                self.params.corrosion_amount.value(),
                &self.corrosion,
                &self.params,
            );

            // Filter — PostAll position: applied on the stereo bus after Corrosion.
            // Acts as a master filter / tone control on the full processed signal.
            if filter_active && filter_position == FilterPosition::PostAll {
                let cutoff    = self.params.filter_cutoff.smoothed.next();
                let resonance = self.params.filter_resonance.smoothed.next();
                let (fl, fr) = self.filter_engine.process_stereo(
                    out_l, out_r, self.sample_rate, filter_type,
                    cutoff, resonance, filter_env_amount,
                    filter_env_attack, filter_env_decay, filter_env_sustain, filter_env_release,
                    filter_drive, filter_midi_note, filter_key_track,
                );
                out_l = fl;
                out_r = fr;
            }

            let abs_l = out_l.abs();
            if abs_l > max_amplitude_in_block_l {
                max_amplitude_in_block_l = abs_l;
            }
            let abs_r = out_r.abs();
            if abs_r > max_amplitude_in_block_r {
                max_amplitude_in_block_r = abs_r;
            }

            // Write stereo output
            for (ch_idx, sample) in channel_samples.into_iter().enumerate() {
                *sample = if ch_idx == 0 { out_l } else { out_r };
            }
        }

        update_peak_meters(
            self.params.editor_state.is_open(),
            buffer.samples() as f32,
            self.meter_decay_per_sample,
            &self.peak_meter_l,
            &self.peak_meter_r,
            max_amplitude_in_block_l,
            max_amplitude_in_block_r,
        );

        ProcessStatus::KeepAlive
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(
            self.params.clone(),
            self.peak_meter_l.clone(),
            self.peak_meter_r.clone(),
            self.params.editor_state.clone(),
        )
    }
}

impl KickSynth {
    fn compute_voice_sample(
        voice: &mut VoiceState,
        params: &SmoothedParams,
        sample_rate: f32,
        free_rng_state: &Cell<u32>,
        wavetable: &[f32],
        sampled_noise: &[f32],
    ) -> f32 {
        if voice.current_phase != EnvelopePhase::Idle || voice.tex_env_phase != EnvelopePhase::Idle
        {
            // ADSR Logic
            match voice.current_phase {
                EnvelopePhase::Attack => {
                    let attack_samples = (sample_rate * (params.attack / 1000.0)).max(1.0);
                    voice.envelope_value += 1.0 / attack_samples;
                    if voice.envelope_value >= 1.0 {
                        voice.envelope_value = 1.0;
                        voice.current_phase = EnvelopePhase::Decay;
                        voice.phase_timer = 0.0;
                    }
                }
                EnvelopePhase::Decay => {
                    let decay_samples = (sample_rate * (params.decay / 1000.0)).max(1.0);
                    let target = params.sustain;
                    voice.envelope_value -= (1.0 - target) / decay_samples;

                    if voice.envelope_value <= target {
                        voice.envelope_value = target;
                        if target <= 0.0 {
                            voice.current_phase = EnvelopePhase::Idle;
                        } else {
                            voice.current_phase = EnvelopePhase::Sustain;
                            voice.phase_timer = 0.0;
                        }
                    }
                }
                EnvelopePhase::Sustain => {
                    voice.envelope_value = params.sustain;
                }
                EnvelopePhase::Release => {
                    if voice.phase_timer == 0.0 {
                        let release_ms = if voice.fast_release {
                            5.0
                        } else {
                            params.release
                        };
                        let release_samples = (sample_rate * (release_ms / 1000.0)).max(1.0);
                        voice.release_coeff = 0.0001f32.powf(1.0 / release_samples);
                    }

                    voice.envelope_value *= voice.release_coeff;
                    voice.phase_timer += 1.0;

                    if voice.envelope_value < 1e-6 {
                        voice.envelope_value = 0.0;
                        voice.current_phase = EnvelopePhase::Idle;
                    }
                }
                _ => {}
            }

            // Pitch Envelope
            let pitch_t = voice.pitch_env_timer / (sample_rate * (params.pitch_decay / 1000.0));
            let pitch_env_val = if pitch_t < 1.0 {
                (1.0 - pitch_t).powf(3.0)
            } else {
                0.0
            };

            let base_tune = if params.bass_synth_mode {
                // MIDI note to frequency
                440.0 * 2.0f32.powf((voice.midi_note as f32 - 69.0) / 12.0)
            } else {
                params.tune
            };

            let current_freq = base_tune
                + (voice.analog_drift * params.analog_variation)
                + (params.sweep * pitch_env_val);
            let phase_inc = current_freq / sample_rate;
            voice.phase = (voice.phase + phase_inc).fract();

            // Sub mode (5) needs an independent half-speed phase that must *not*
            // reset on every fundamental cycle — hence the dedicated counter.
            if params.waveform == 5 {
                voice.waveform_phase2 = (voice.waveform_phase2 + phase_inc * 0.5).fract();
            }

            let pi2 = 2.0 * std::f32::consts::PI;

            // ── Waveform modes ────────────────────────────────────────────────
            // All modes are normalised to peak ±1.0 so downstream gain staging
            // is identical regardless of which mode is selected.
            let osc_out = match params.waveform {
                // 1 · Pure Sine — clean reference
                1 => (voice.phase * pi2).sin(),

                // 2 · Octave — sine + octave above (2× freq) at 50 %
                //     Adds presence and punch in the 80–200 Hz sweep range.
                //     Both partials track the pitch envelope together.
                2 => {
                    let base = (voice.phase * pi2).sin();
                    let oct  = (voice.phase * pi2 * 2.0).sin();
                    (base + oct * 0.3) / 1.5
                }

                // 3 · Fifth — sine + perfect fifth (1.5× freq) at 35 %
                //     The fifth interval gives a tuned, musical sub character
                //     without introducing harsh high harmonics.
                3 => {
                    let base  = (voice.phase * pi2).sin();
                    let fifth = (voice.phase * pi2 * 1.5).sin();
                    (base + fifth * 0.35) / 1.35
                }

                // 4 · Warm — tanh-saturated sine (k = 2.0)
                //     tanh(k·x)/tanh(k) is an odd-symmetric soft clipper, so it
                //     adds only odd harmonics (3rd ≈ 8 %, 5th ≈ 1 %).  At 50 Hz
                //     the 3rd harmonic lands at 150 Hz — right at the low-pass
                //     rolloff the user described.  Sounds like a gently driven
                //     tube stage.
                4 => {
                    let sine = (voice.phase * pi2).sin();
                    let k = 2.0_f32;
                    (k * sine).tanh() / k.tanh()
                }

                // 5 · Sub — sine + sub-octave (0.5× freq) at 40 %
                //     The sub-octave doubles the perceived low-end weight.
                //     Uses the dedicated waveform_phase2 counter so the
                //     half-speed oscillator is never reset mid-cycle.
                5 => {
                    let base = (voice.phase * pi2).sin();
                    let sub  = (voice.waveform_phase2 * pi2).sin();
                    (base + sub * 0.4) / 1.4
                }

                _ => (voice.phase * pi2).sin(),
            };

            let signal = osc_out * voice.envelope_value;

            // Texture Logic
            let mut tex_signal = 0.0;
            match voice.tex_env_phase {
                EnvelopePhase::Decay => {
                    let decay_samples = (sample_rate * (params.tex_decay / 1000.0)).max(1.0);
                    voice.tex_env_value -= 1.0 / decay_samples;
                    if voice.tex_env_value <= 0.0 {
                        voice.tex_env_value = 0.0;
                        voice.tex_env_phase = EnvelopePhase::Idle;
                    }
                }
                EnvelopePhase::Release => {
                    let release_samples = (sample_rate * (10.0 / 1000.0)).max(1.0); // 10ms release
                    voice.tex_env_value -= 1.0 / release_samples;
                    if voice.tex_env_value <= 0.0 {
                        voice.tex_env_value = 0.0;
                        voice.tex_env_phase = EnvelopePhase::Idle;
                    }
                }
                _ => {}
            }

            if voice.tex_env_phase != EnvelopePhase::Idle && params.tex_amt > 0.0 {
                voice.static_rng_state = voice
                    .static_rng_state
                    .wrapping_mul(1664525)
                    .wrapping_add(1013904223);

                let current_free_rng = free_rng_state.get();
                free_rng_state.set(
                    current_free_rng
                        .wrapping_mul(1664525)
                        .wrapping_add(1013904223),
                );

                let static_val = (voice.static_rng_state as f32) / (u32::MAX as f32);
                let free_val = (current_free_rng as f32) / (u32::MAX as f32);

                let static_sym = static_val * 2.0 - 1.0;
                let free_sym = free_val * 2.0 - 1.0;

                let noise_val = match params.tex_type {
                    1 => {
                        // DUST
                        let inverted_tone = 1.0 - params.tex_tone;
                        let threshold = 0.999 - (inverted_tone * 0.1);
                        let static_dust = if static_val > threshold {
                            static_sym
                        } else {
                            0.0
                        };
                        let free_dust = if free_val > threshold { free_sym } else { 0.0 };
                        let raw_dust = static_dust * (1.0 - params.tex_variation)
                            + free_dust * params.tex_variation;

                        let cutoff = 8000.0 * (1.0 - inverted_tone * 0.9);
                        let dt = 1.0 / sample_rate;
                        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
                        let alpha = dt / (rc + dt);

                        voice.tex_filter_state += alpha * (raw_dust - voice.tex_filter_state);
                        voice.tex_filter_state * 3.0
                    }
                    2 => {
                        // CRACKLE
                        let cutoff = 200.0 * 50.0_f32.powf(params.tex_tone);
                        let dt = 1.0 / sample_rate;
                        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
                        let alpha = dt / (rc + dt);

                        let eq_pwr_static = (1.0 - params.tex_variation).sqrt();
                        let eq_pwr_free = params.tex_variation.sqrt();
                        let combined_noise = static_sym * eq_pwr_static + free_sym * eq_pwr_free;

                        voice.tex_filter_state += alpha * (combined_noise - voice.tex_filter_state);
                        let shaped = voice.tex_filter_state
                            * voice.tex_filter_state
                            * voice.tex_filter_state
                            * 10.0;
                        shaped.clamp(-1.0, 1.0)
                    }
                    3 => {
                        // SAMPLED
                        let mut t = voice.wt_phase * (sampled_noise.len() as f32);
                        let playback_speed = 0.5 + (8.0_f32 * params.tex_tone);

                        voice.wt_phase += playback_speed / sample_rate;
                        if voice.wt_phase >= 1.0 {
                            voice.wt_phase -= 1.0;
                        }
                        if voice.wt_phase < 0.0 {
                            voice.wt_phase += 1.0;
                        }

                        t = t.clamp(0.0, (sampled_noise.len() - 2) as f32);
                        let idx1 = t as usize;
                        let idx2 = idx1 + 1;
                        let frac = t.fract();

                        let s1 = sampled_noise[idx1];
                        let s2 = sampled_noise[idx2];
                        s1 + frac * (s2 - s1)
                    }
                    4 => {
                        // ORGANIC
                        let adjusted_tone = if params.tex_tone < 0.5 {
                            // Map 0.0 -> 0.5 to a wider range
                            // This doubles the "distance" from the center (0.5)
                            0.5 - (0.5 - params.tex_tone) * 1.5
                        } else {
                            // Keep 0.5 -> 1.0 exactly as is
                            params.tex_tone
                        };

                        let base_freq = 20.0 * 50.0_f32.powf(adjusted_tone);
                        let eq_pwr_static = (1.0 - params.tex_variation).sqrt();
                        let eq_pwr_free = params.tex_variation.sqrt();
                        let combined_noise = static_sym * eq_pwr_static + free_sym * eq_pwr_free;

                        let freq = base_freq * (1.0 + 0.05 * combined_noise);
                        let phase_inc = freq / sample_rate;
                        voice.wt_phase = (voice.wt_phase + phase_inc).fract();

                        let wt_len = wavetable.len() as f32;
                        let idx = voice.wt_phase * wt_len;
                        let idx_i = idx as usize;
                        let idx_next = (idx_i + 1) % wavetable.len();
                        let frac = idx.fract();

                        let s1 = wavetable[idx_i];
                        let s2 = wavetable[idx_next];
                        s1 + frac * (s2 - s1)
                    }
                    5 => {
                        // VINYL
                        let eq_pwr_static = (1.0 - params.tex_variation).sqrt();
                        let eq_pwr_free = params.tex_variation.sqrt();
                        let hiss_noise = static_sym * eq_pwr_static + free_sym * eq_pwr_free;

                        let mix_val = static_val * (1.0 - params.tex_variation)
                            + free_val * params.tex_variation;
                        let pop_threshold = 0.9995 - (params.tex_tone * 0.005);
                        let pop = if mix_val > pop_threshold {
                            1.5 * hiss_noise.signum()
                        } else {
                            0.0
                        };

                        let cutoff = 4000.0;
                        let dt = 1.0 / sample_rate;
                        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
                        let alpha = dt / (rc + dt);
                        voice.tex_filter_state += alpha * (hiss_noise - voice.tex_filter_state);

                        let noise = voice.tex_filter_state * 0.3 + pop;
                        noise.clamp(-1.0, 1.0)
                    }
                    6 => {
                        // ZAP
                        let freq = 1000.0 * 4.0_f32.powf((2.0_f32 * params.tex_tone));
                        let mut mod_freq = freq * 0.5 * params.tex_variation;
                        if params.tex_variation <= 0.0 {
                            mod_freq = freq * 0.5;
                        }

                        let drift = (free_val - 0.5) * 200.0 * params.tex_variation;
                        voice.tex_filter_state_2 =
                            (voice.tex_filter_state_2 + (mod_freq + drift) / sample_rate).fract();
                        let mo = (voice.tex_filter_state_2 * 2.0 * std::f32::consts::PI).sin();

                        voice.wt_phase =
                            (voice.wt_phase + (freq + mo * 1000.0) / sample_rate).fract();
                        (voice.wt_phase * 2.0 * std::f32::consts::PI).sin() * 0.6
                    }
                    _ => 0.0,
                };
                tex_signal = noise_val * voice.tex_env_value * params.tex_amt * 0.4 * 0.7;
            }

            let pre_drive = signal + tex_signal;

            let driven_signal = match params.drive_model {
                1 => Self::drive_tape_classic(params.drive, pre_drive) * 0.89125,
                2 => Self::drive_tape_modern(params.drive, pre_drive) * 1.412,
                3 => Self::drive_tube_triode(params.drive, pre_drive) * 0.917,
                4 => Self::drive_tube_pentode(params.drive, pre_drive) * 0.79433,
                5 => {
                    let scaled_drive = params.drive * 0.59;
                    Self::drive_saturation_digital(scaled_drive, pre_drive) * 0.729
                }
                _ => Self::drive_tape_classic(params.drive, pre_drive) * 0.89125,
            };

            let drive_wet = params.drive.sqrt();
            let driven = pre_drive * (1.0 - drive_wet) + driven_signal * drive_wet;

            let vel_range_db = 10.0;
            let min_gain = 10.0f32.powf(-vel_range_db / 20.0);
            let vel_mapped = min_gain + voice.midi_velocity * (1.0 - min_gain);

            let output_val = driven * vel_mapped * 0.5;

            if voice.current_phase != EnvelopePhase::Idle {
                voice.pitch_env_timer += 1.0;
                voice.phase_timer += 1.0;
            }
            output_val
        } else {
            0.0
        }
    }

    fn trigger_note(&mut self, velocity: f32, note: u8) {
        if self.voice.current_phase != EnvelopePhase::Idle {
            let mut releasing = self.voice.clone();
            releasing.current_phase = EnvelopePhase::Release;
            if releasing.tex_env_phase != EnvelopePhase::Idle {
                releasing.tex_env_phase = EnvelopePhase::Release;
            }
            releasing.phase_timer = 0.0;
            releasing.fast_release = true;
            self.releasing_voice = Some(releasing);
        }

        self.voice = VoiceState::default();
        self.voice.midi_note = note;
        self.voice.midi_velocity = velocity;
        self.voice.current_phase = EnvelopePhase::Attack;

        let current_rng = self.free_rng_state.get();
        let next_rng = current_rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.free_rng_state.set(next_rng);

        self.voice.analog_drift = ((next_rng as f32 / u32::MAX as f32) * 2.0 - 1.0) * 1.5;

        self.voice.tex_env_phase = EnvelopePhase::Decay;
        self.voice.tex_env_value = 1.0;
        // Set trigger/gate mode from the param, then fire the filter envelope.
        // Always triggered regardless of whether the filter is currently active
        // so it's already in sync if the user enables it mid-session.
        self.filter_engine.envelope.mode = if self.params.filter_env_trigger.value() {
            crate::filter::FilterEnvMode::Trigger
        } else {
            crate::filter::FilterEnvMode::Gate
        };
        self.filter_engine.trigger(velocity);
    }

    fn release_note(&mut self) {
        if self.voice.current_phase != EnvelopePhase::Idle
            && self.voice.current_phase != EnvelopePhase::Release
        {
            self.voice.current_phase = EnvelopePhase::Release;
            self.voice.phase_timer = 0.0;
            self.voice.fast_release = false;
        }
        if self.voice.tex_env_phase != EnvelopePhase::Idle
            && self.voice.tex_env_phase != EnvelopePhase::Release
        {
            self.voice.tex_env_phase = EnvelopePhase::Release;
        }
        // In trigger mode the filter envelope completes on its own — note-off
        // must not interrupt it or the filter would snap shut on short notes.
        if !self.params.filter_env_trigger.value() {
            self.filter_engine.release();
        }
    }

    fn drive_tape_classic(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 12.0;
        let x = signal * gain;
        let saturated = if x.abs() < 0.5 {
            x * (1.0 - 0.15 * x.abs())
        } else {
            let sign = x.signum();
            sign * (0.425 + 0.575 * (1.0 - (-(x.abs() - 0.5) * 3.0).exp()))
        };
        saturated * 0.85
    }

    fn drive_tape_modern(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 10.0;
        let x = signal * gain;
        let saturated = if x >= 0.0 {
            x / (1.0 + x.abs().powf(1.4))
        } else {
            x / (1.0 + (x.abs() * 0.85).powf(1.2))
        };
        saturated * 0.9
    }

    fn drive_tube_triode(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 15.0;
        let x = signal * gain;
        let saturated = if x.abs() < 0.1 {
            x * 0.95
        } else {
            let sign = x.signum();
            let abs_x = x.abs();
            let mapped = (abs_x - 0.1) * 1.5;
            sign * (0.095 + mapped.tanh() * 0.905)
        };
        saturated * 0.75
    }

    fn drive_tube_pentode(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 18.0;
        let x = signal * gain;
        let saturated = if x.abs() < 0.3 {
            x
        } else {
            let sign = x.signum();
            let abs_x = x.abs();
            let mapped = (abs_x - 0.3) * 2.0;
            sign * (0.3 + mapped.atan() * (0.7 / std::f32::consts::FRAC_PI_2))
        };
        saturated * 0.7
    }

    fn drive_saturation_digital(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 20.0;
        let x = signal * gain;
        let saturated = if x.abs() <= 1.0 {
            x * (1.5 - 0.5 * x.abs().powf(2.0))
        } else {
            x.signum()
        };
        let threshold = 0.8;
        let final_signal = if saturated.abs() > threshold {
            let sign = saturated.signum();
            let over = saturated.abs() - threshold;
            sign * (threshold + over / (1.0 + over * 3.0))
        } else {
            saturated
        };
        final_signal * 0.8
    }

    fn corrosion_read(buf: &[f32], write_pos: usize, delay_samples: f32) -> f32 {
        let len = buf.len();
        let delay_i = delay_samples as usize;
        let frac = delay_samples - delay_i as f32;
        let delay_i = delay_i.min(len - 1);
        let idx0 = (write_pos + len - delay_i) % len;
        let idx1 = (write_pos + len - delay_i - 1) % len;
        buf[idx0] + frac * (buf[idx1] - buf[idx0])
    }

    fn apply_corrosion(
        sample_rate: f32,
        driven: f32,
        corr_amount: f32,
        corrosion: &RefCell<CorrosionState>,
        kick_params: &Arc<KickParams>,
    ) -> (f32, f32) {
        if corr_amount > 0.0 {
            let corr_freq_norm = kick_params.corrosion_frequency.smoothed.next();
            let corr_freq = log_scale(corr_freq_norm, 15.0, 22000.0);
            let corr_width = kick_params.corrosion_width.smoothed.next();
            let corr_blend = kick_params.corrosion_noise_blend.smoothed.next();
            let corr_stereo = kick_params.corrosion_stereo.smoothed.next();

            const BASE_DELAY: f32 = 0.002;
            const MAX_MOD_DEPTH: f32 = 0.001;

            let delay_samples_l;
            let delay_samples_r;
            let write_pos;

            {
                let mut state = corrosion.borrow_mut();

                let sine_l = (state.sine_phase * std::f32::consts::TAU).sin();
                let sine_r = ((state.sine_phase * std::f32::consts::TAU)
                    + corr_stereo * std::f32::consts::PI)
                    .sin();
                state.sine_phase = (state.sine_phase + corr_freq / sample_rate).fract();

                state.rng = state
                    .rng
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                let raw_noise_l = (state.rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
                state.rng = state
                    .rng
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                let raw_noise_r = (state.rng as f32 / u32::MAX as f32) * 2.0 - 1.0;

                let lp_cutoff = (corr_freq * corr_width.max(0.01)).min(sample_rate * 0.499);
                let hp_cutoff = (corr_freq / corr_width.max(0.01).max(1.0)).max(1.0);
                let dt = 1.0 / sample_rate;
                let lp_a = dt / (1.0 / (std::f32::consts::TAU * lp_cutoff) + dt);
                let hp_a = dt / (1.0 / (std::f32::consts::TAU * hp_cutoff) + dt);

                state.bp_l[0] += lp_a * (raw_noise_l - state.bp_l[0]);
                let lp_l = state.bp_l[0];
                state.bp_l[1] += hp_a * (lp_l - state.bp_l[1]);
                let bp_l = lp_l - state.bp_l[1];

                state.bp_r[0] += lp_a * (raw_noise_r - state.bp_r[0]);
                let lp_r = state.bp_r[0];
                state.bp_r[1] += hp_a * (lp_r - state.bp_r[1]);
                let bp_r = lp_r - state.bp_r[1];

                let noise_l = bp_l;
                let noise_r = noise_l + corr_stereo * (bp_r - noise_l);

                let mod_l = sine_l + corr_blend * (noise_l - sine_l);
                let mod_r = sine_r + corr_blend * (noise_r - sine_r);

                delay_samples_l =
                    (BASE_DELAY + mod_l * corr_amount * MAX_MOD_DEPTH).max(0.0) * sample_rate;
                delay_samples_r =
                    (BASE_DELAY + mod_r * corr_amount * MAX_MOD_DEPTH).max(0.0) * sample_rate;

                write_pos = state.write;
                let buf_len = state.buf_l.len();
                state.buf_l[write_pos] = driven;
                state.buf_r[write_pos] = driven;

                state.write = (write_pos + 1) % buf_len;
            }

            let state = corrosion.borrow();
            let read_l = Self::corrosion_read(&state.buf_l, write_pos, delay_samples_l);
            let read_r = Self::corrosion_read(&state.buf_r, write_pos, delay_samples_r);

            (read_l, read_r)
        } else {
            (driven, driven)
        }
    }
}

#[inline]
fn update_peak_meters(
    editor_open: bool,
    buffer_samples: f32,
    meter_decay_per_sample: f32,
    peak_meter_l: &Arc<AtomicF32>,
    peak_meter_r: &Arc<AtomicF32>,
    max_amplitude_l: f32,
    max_amplitude_r: f32,
) {
    if !editor_open {
        return;
    }

    let block_decay = f32::powf(meter_decay_per_sample, buffer_samples);

    let current_peak_l = peak_meter_l.load(Ordering::Relaxed);
    let mut new_peak_l = if max_amplitude_l > current_peak_l {
        max_amplitude_l
    } else {
        current_peak_l * block_decay
    };
    if new_peak_l < 0.001 {
        new_peak_l = 0.0;
    }
    peak_meter_l.store(new_peak_l, Ordering::Relaxed);

    let current_peak_r = peak_meter_r.load(Ordering::Relaxed);
    let mut new_peak_r = if max_amplitude_r > current_peak_r {
        max_amplitude_r
    } else {
        current_peak_r * block_decay
    };
    if new_peak_r < 0.001 {
        new_peak_r = 0.0;
    }
    peak_meter_r.store(new_peak_r, Ordering::Relaxed);
}

impl Vst3Plugin for KickSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"BrgrKickSynthV01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(KickSynth);
