/*
 * Copyright (C) 2026 Marinus Burger
 */

use nih_plug::prelude::*;
use nih_plug_vizia::ViziaState;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod editor;

// Helper function to map a 0-1 value to a logarithmic frequency range
fn log_scale(value: f32, min: f32, max: f32) -> f32 {
    min * (max / min).powf(value)
}

pub struct KickSynth {
    params: Arc<KickParams>,
    sample_rate: f32,

    // DSP State
    phase: f32,
    #[cfg(debug_assertions)]
    debug_phase: f32,
    #[cfg(debug_assertions)]
    debug_release_timer: f32,

    // ADSR State
    envelope_value: f32,
    current_phase: EnvelopePhase,
    phase_timer: f32,
    release_coeff: f32,

    // Pitch Envelope State
    pitch_env_timer: f32,

    // Texture State
    tex_env_value: f32,
    tex_env_phase: EnvelopePhase,
    tex_release_early: bool,
    wt_phase: f32,
    tex_filter_state: f32,
    tex_filter_state_2: f32,
    wavetable: Vec<f32>,
    sampled_noise: Vec<f32>,
    static_rng_state: u32,
    free_rng_state: u32,

    // Corrosion (Erosion-style phase-modulated delay) State
    corrosion_buf_l: Vec<f32>,
    corrosion_buf_r: Vec<f32>,
    corrosion_write: usize,
    corrosion_sine_phase: f32,
    // Bandpass filter states for noise modulator (two 1-pole stages each channel)
    corrosion_bp_l: [f32; 2],
    corrosion_bp_r: [f32; 2],
    // Small LCG for corrosion noise generation
    corrosion_rng: u32,

    meter_decay_per_sample: f32,
    peak_meter_l: Arc<AtomicF32>,
    peak_meter_r: Arc<AtomicF32>,

    // Trigger Logic
    was_trigger_on: bool,
    midi_velocity: f32,
    active_midi_note: Option<u8>,
    analog_drift: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvelopePhase {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Params)]
struct KickParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<ViziaState>,

    /// The main "Tune" of the kick (Hz)
    #[id = "tune"]
    pub tune: FloatParam,

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
    pub gui_trigger: AtomicBool,
    /// Release Logic for the UI (consumed by audio thread)
    pub gui_release: AtomicBool,
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),

            gui_trigger: AtomicBool::new(false),
            gui_release: AtomicBool::new(false),

            tune: FloatParam::new(
                "Tune",
                44.0,
                FloatRange::Linear {
                    min: 30.0,
                    max: 150.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} hz", value))),

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

            drive: FloatParam::new("Gain", 0.33, FloatRange::Linear { min: 0.0, max: 1.0 })
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

            tex_amt: FloatParam::new("Tex Amount", 0.2, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            tex_decay: FloatParam::new(
                "Tex Decay",
                80.0,
                FloatRange::Linear {
                    min: 5.0,
                    max: 650.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} ms", value))),

            tex_variation: FloatParam::new(
                "Tex Variation",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            analog_variation: FloatParam::new(
                "Analog Instability",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            tex_type: IntParam::new(
                "Tex Type",
                1i32,
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

            tex_tone: FloatParam::new("Tex Tone", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            attack: FloatParam::new(
                "[A]",
                0.1,
                FloatRange::Linear {
                    min: 0.1,
                    max: 100.0,
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
                    max: 1000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} ms", value))),

            corrosion_frequency: FloatParam::new(
                "Corrosion Freq",
                0.5, // Default to a middle value in the normalized range
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(move |value| {
                format!("{:.0} hz", log_scale(value, 15.0, 22000.0))
            })),

            corrosion_width: FloatParam::new(
                "Corrosion Width",
                0.5,
                FloatRange::Linear { min: 0.1, max: 2.5 },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1}", value))),

            corrosion_noise_blend: FloatParam::new(
                "Corrosion Sine ~ Noise",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            corrosion_stereo: FloatParam::new(
                "Corrosion Stereo",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            corrosion_amount: FloatParam::new(
                "Corrosion Amount",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0)),

            trigger: BoolParam::new("Trigger", false),
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
            phase: 0.0,
            #[cfg(debug_assertions)]
            debug_phase: 0.0,
            #[cfg(debug_assertions)]
            debug_release_timer: -1.0,
            envelope_value: 0.0,
            current_phase: EnvelopePhase::Idle,
            phase_timer: 0.0,
            release_coeff: 0.0,
            pitch_env_timer: 0.0,

            tex_env_value: 0.0,
            tex_env_phase: EnvelopePhase::Idle,
            tex_release_early: false,
            wt_phase: 0.0,
            tex_filter_state: 0.0,
            tex_filter_state_2: 0.0,
            wavetable,
            sampled_noise,
            static_rng_state: 1337,
            free_rng_state: 80085,

            corrosion_buf_l: vec![0.0; corrosion_buf_size],
            corrosion_buf_r: vec![0.0; corrosion_buf_size],
            corrosion_write: 0,
            corrosion_sine_phase: 0.0,
            corrosion_bp_l: [0.0; 2],
            corrosion_bp_r: [0.0; 2],
            corrosion_rng: 0xDEAD_BEEF,

            meter_decay_per_sample: 1.0,
            peak_meter_l: Arc::new(AtomicF32::new(0.0)), // 0.0 Linear = Silence
            peak_meter_r: Arc::new(AtomicF32::new(0.0)),

            was_trigger_on: false,
            midi_velocity: 1.0,
            active_midi_note: None,
            analog_drift: 0.0,
        }
    }
}

impl Plugin for KickSynth {
    const NAME: &'static str = "Kick Synth";
    const VENDOR: &'static str = "Convolution DEV";
    const URL: &'static str = "https://github.com/minburg/vst-kick-synth";
    const EMAIL: &'static str = "email@example.com";
    const VERSION: &'static str = "0.1.0";

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
        // We need at least (base_delay + max_mod_depth) * sample_rate samples:
        //   2ms base + 1ms max depth = 3ms => sample_rate * 0.003, rounded up with margin.
        let corrosion_buf_size =
            ((_buffer_config.sample_rate * 0.004) as usize + 4).next_power_of_two();
        self.corrosion_buf_l = vec![0.0; corrosion_buf_size];
        self.corrosion_buf_r = vec![0.0; corrosion_buf_size];
        self.corrosion_write = 0;

        let release_db_per_second = 160.0;

        // Calculate the constant for 1 sample of decay
        // We store this in the struct
        self.meter_decay_per_sample = f32::powf(
            10.0,
            -release_db_per_second / (20.0 * _buffer_config.sample_rate),
        );

        true
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        #[cfg(debug_assertions)]
        {
            self.debug_phase = 0.0;
            self.debug_release_timer = -1.0;
        }
        self.envelope_value = 0.0;
        self.current_phase = EnvelopePhase::Idle;
        self.release_coeff = 0.0;
        self.tex_env_value = 0.0;
        self.tex_env_phase = EnvelopePhase::Idle;
        self.was_trigger_on = false;
        self.tex_release_early = false;
        self.active_midi_note = None;

        // Clear corrosion state
        self.corrosion_buf_l.iter_mut().for_each(|s| *s = 0.0);
        self.corrosion_buf_r.iter_mut().for_each(|s| *s = 0.0);
        self.corrosion_write = 0;
        self.corrosion_sine_phase = 0.0;
        self.corrosion_bp_l = [0.0; 2];
        self.corrosion_bp_r = [0.0; 2];
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _ctx: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let sample_rate = self.sample_rate;
        let mut next_event = _ctx.next_event();

        let mut max_amplitude_in_block_l: f32 = 0.0;
        let mut max_amplitude_in_block_r: f32 = 0.0;

        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {
            #[cfg(debug_assertions)]
            {
                self.debug_phase += 1.0 / sample_rate;
                if self.debug_phase >= 0.5 {
                    self.debug_phase -= 0.5;
                    self.trigger_note(0.8);
                    self.debug_release_timer = 0.2 * sample_rate;
                }

                if self.debug_release_timer > 0.0 {
                    self.debug_release_timer -= 1.0;
                    if self.debug_release_timer <= 0.0 {
                        self.release_note();
                    }
                }
            }

            // 1. Trigger Check (UI Atomic OR Parameter Edge)
            let gui_triggered = self.params.gui_trigger.swap(false, Ordering::SeqCst);
            let gui_released = self.params.gui_release.swap(false, Ordering::SeqCst);
            let trigger_param_val = self.params.trigger.value();

            if gui_triggered || (trigger_param_val && !self.was_trigger_on) {
                self.trigger_note(1.0);
            } else if gui_released && self.was_trigger_on {
                self.release_note();
            }
            self.was_trigger_on = trigger_param_val;

            // 2. MIDI Handle
            while let Some(event) = next_event {
                if event.timing() > sample_idx as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { velocity, note, .. } => {
                        // Accept any note on any channel
                        if velocity > 0.0 {
                            self.active_midi_note = Some(note);
                            self.trigger_note(velocity);
                        } else {
                            if self.active_midi_note == Some(note) {
                                self.release_note();
                                self.active_midi_note = None;
                            }
                        }
                    }
                    NoteEvent::NoteOff { note, .. } => {
                        // Accept any note off on any channel
                        if self.active_midi_note == Some(note) {
                            self.release_note();
                            self.active_midi_note = None;
                        }
                    }
                    _ => (),
                }
                next_event = _ctx.next_event();
            }

            // 3. DSP – stereo output pair (L and R may differ due to Corrosion stereo)
            let mut out_l = 0.0_f32;
            let mut out_r = 0.0_f32;

            if self.current_phase != EnvelopePhase::Idle
                || self.tex_env_phase != EnvelopePhase::Idle
            {
                let base_freq = self.params.tune.smoothed.next();
                let sweep_amt = self.params.sweep.smoothed.next();
                let pitch_decay_ms = self.params.pitch_decay.smoothed.next();
                let drive = self.params.drive.smoothed.next();
                let drive_model = self.params.drive_model.value();

                let tex_amt = self.params.tex_amt.smoothed.next();
                let tex_decay_ms = self.params.tex_decay.smoothed.next();
                let rand_param = self.params.tex_variation.smoothed.next();
                let analog_param = self.params.analog_variation.smoothed.next();
                let tex_type = self.params.tex_type.value();
                let tex_tone = self.params.tex_tone.smoothed.next();

                let attack_ms = self.params.attack.smoothed.next();
                let decay_ms = self.params.decay.smoothed.next();
                let sustain_lvl = self.params.sustain.smoothed.next();
                let release_ms = self.params.release.smoothed.next();

                // ADSR Logic
                match self.current_phase {
                    EnvelopePhase::Attack => {
                        let attack_samples = (sample_rate * (attack_ms / 1000.0)).max(1.0);
                        self.envelope_value += 1.0 / attack_samples;
                        if self.envelope_value >= 1.0 {
                            self.envelope_value = 1.0;
                            self.current_phase = EnvelopePhase::Decay;
                            self.phase_timer = 0.0;
                        }
                    }
                    EnvelopePhase::Decay => {
                        let decay_samples = (sample_rate * (decay_ms / 1000.0)).max(1.0);
                        let target = sustain_lvl;
                        self.envelope_value -= (1.0 - target) / decay_samples;

                        if self.envelope_value <= target {
                            self.envelope_value = target;
                            if target <= 0.0 {
                                self.current_phase = EnvelopePhase::Idle;
                            } else {
                                self.current_phase = EnvelopePhase::Sustain;
                                self.phase_timer = 0.0;
                            }
                        }
                    }
                    EnvelopePhase::Sustain => {
                        self.envelope_value = sustain_lvl;
                    }
                    EnvelopePhase::Release => {
                        if self.phase_timer == 0.0 {
                            let release_samples = (sample_rate * (release_ms / 1000.0)).max(1.0);
                            self.release_coeff = (0.0001f32).powf(1.0 / release_samples);
                        }

                        self.envelope_value *= self.release_coeff;
                        self.phase_timer += 1.0;

                        if self.envelope_value < 1e-6 {
                            self.envelope_value = 0.0;
                            self.current_phase = EnvelopePhase::Idle;
                        }
                    }
                    _ => {}
                }

                // Pitch Envelope
                let pitch_t = self.pitch_env_timer / (sample_rate * (pitch_decay_ms / 1000.0));
                let pitch_env_val = if pitch_t < 1.0 {
                    (1.0 - pitch_t).powf(3.0)
                } else {
                    0.0
                };

                let current_freq =
                    base_freq + (self.analog_drift * analog_param) + (sweep_amt * pitch_env_val);
                let phase_inc = current_freq / sample_rate;
                self.phase = (self.phase + phase_inc).fract();

                let sine_wave = (self.phase * 2.0 * std::f32::consts::PI).sin();
                let signal = sine_wave * self.envelope_value;

                // Texture Logic
                let mut tex_signal = 0.0;
                match self.tex_env_phase {
                    EnvelopePhase::Decay => {
                        let decay_samples = (sample_rate * (tex_decay_ms / 1000.0)).max(1.0);
                        self.tex_env_value -= 1.0 / decay_samples;
                        if self.tex_env_value <= 0.0 {
                            self.tex_env_value = 0.0;
                            self.tex_env_phase = EnvelopePhase::Idle;
                        }
                    }
                    EnvelopePhase::Release => {
                        let release_samples = (sample_rate * (10.0 / 1000.0)).max(1.0); // 10ms release
                        self.tex_env_value -= 1.0 / release_samples;
                        if self.tex_env_value <= 0.0 {
                            self.tex_env_value = 0.0;
                            self.tex_env_phase = EnvelopePhase::Idle;
                        }
                    }
                    _ => {}
                }

                if self.tex_env_phase != EnvelopePhase::Idle && tex_amt > 0.0 {
                    self.static_rng_state = self
                        .static_rng_state
                        .wrapping_mul(1664525)
                        .wrapping_add(1013904223);
                    self.free_rng_state = self
                        .free_rng_state
                        .wrapping_mul(1664525)
                        .wrapping_add(1013904223);

                    let static_val = (self.static_rng_state as f32) / (u32::MAX as f32);
                    let free_val = (self.free_rng_state as f32) / (u32::MAX as f32);

                    let static_sym = static_val * 2.0 - 1.0;
                    let free_sym = free_val * 2.0 - 1.0;

                    let noise_val = match tex_type {
                        1 => {
                            let threshold = 0.999 - (tex_tone * 0.1);
                            let static_dust = if static_val > threshold {
                                static_sym
                            } else {
                                0.0
                            };
                            let free_dust = if free_val > threshold { free_sym } else { 0.0 };
                            let raw_dust =
                                static_dust * (1.0 - rand_param) + free_dust * rand_param;

                            // Lowpass filter explicitly mapped by tex_tone
                            let cutoff = 8000.0 * (1.0 - tex_tone * 0.9);
                            let dt = 1.0 / sample_rate;
                            let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
                            let alpha = dt / (rc + dt);

                            self.tex_filter_state += alpha * (raw_dust - self.tex_filter_state);
                            self.tex_filter_state * 3.0
                        }
                        2 => {
                            let cutoff = 200.0 * (50.0_f32).powf(tex_tone);
                            let dt = 1.0 / sample_rate;
                            let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
                            let alpha = dt / (rc + dt);

                            let eq_pwr_static = (1.0 - rand_param).sqrt();
                            let eq_pwr_free = rand_param.sqrt();
                            let combined_noise =
                                static_sym * eq_pwr_static + free_sym * eq_pwr_free;

                            self.tex_filter_state +=
                                alpha * (combined_noise - self.tex_filter_state);
                            let shaped = self.tex_filter_state
                                * self.tex_filter_state
                                * self.tex_filter_state
                                * 10.0;
                            shaped.clamp(-1.0, 1.0)
                        }
                        3 => {
                            // Sampled Noise playback
                            let mut t = self.wt_phase * (self.sampled_noise.len() as f32);
                            let playback_speed = 0.5 + tex_tone;

                            self.wt_phase += playback_speed / sample_rate;
                            if self.wt_phase >= 1.0 {
                                self.wt_phase -= 1.0;
                            }
                            if self.wt_phase < 0.0 {
                                self.wt_phase += 1.0;
                            }

                            t = t.clamp(0.0, (self.sampled_noise.len() - 2) as f32);
                            let idx1 = t as usize;
                            let idx2 = idx1 + 1;
                            let frac = t.fract();

                            let s1 = self.sampled_noise[idx1];
                            let s2 = self.sampled_noise[idx2];
                            s1 + frac * (s2 - s1)
                        }
                        4 => {
                            let base_freq = 20.0 * (50.0_f32).powf(tex_tone);
                            let eq_pwr_static = (1.0 - rand_param).sqrt();
                            let eq_pwr_free = rand_param.sqrt();
                            let combined_noise =
                                static_sym * eq_pwr_static + free_sym * eq_pwr_free;

                            let freq = base_freq * (1.0 + 0.05 * combined_noise);
                            let phase_inc = freq / sample_rate;
                            self.wt_phase = (self.wt_phase + phase_inc).fract();

                            let wt_len = self.wavetable.len() as f32;
                            let idx = self.wt_phase * wt_len;
                            let idx_i = idx as usize;
                            let idx_next = (idx_i + 1) % self.wavetable.len();
                            let frac = idx.fract();

                            let s1 = self.wavetable[idx_i];
                            let s2 = self.wavetable[idx_next];
                            s1 + frac * (s2 - s1)
                        }
                        5 => {
                            // Vinyl Hiss & Pop
                            let eq_pwr_static = (1.0 - rand_param).sqrt();
                            let eq_pwr_free = rand_param.sqrt();
                            let hiss_noise = static_sym * eq_pwr_static + free_sym * eq_pwr_free;

                            // Pop generator
                            let mix_val = static_val * (1.0 - rand_param) + free_val * rand_param;
                            let pop_threshold = 0.9995 - (tex_tone * 0.005);
                            let pop = if mix_val > pop_threshold {
                                1.5 * hiss_noise.signum()
                            } else {
                                0.0
                            };

                            // Soft filter on hiss
                            let cutoff = 4000.0;
                            let dt = 1.0 / sample_rate;
                            let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
                            let alpha = dt / (rc + dt);
                            self.tex_filter_state += alpha * (hiss_noise - self.tex_filter_state);

                            let noise = self.tex_filter_state * 0.3 + pop;
                            noise.clamp(-1.0, 1.0)
                        }
                        6 => {
                            // Electrical Zap/FM style burst
                            let freq = 1000.0 * (4.0_f32).powf(tex_tone);
                            let mut mod_freq = freq * 0.5 * rand_param;
                            if rand_param <= 0.0 {
                                mod_freq = freq * 0.5; // Ensure mode is interesting at 0.0 randomness
                            }

                            // Free-running modulator
                            let drift = (free_val - 0.5) * 200.0 * rand_param;
                            self.tex_filter_state_2 = (self.tex_filter_state_2
                                + (mod_freq + drift) / sample_rate)
                                .fract();
                            let mo = (self.tex_filter_state_2 * 2.0 * std::f32::consts::PI).sin();

                            self.wt_phase =
                                (self.wt_phase + (freq + mo * 1000.0) / sample_rate).fract();
                            (self.wt_phase * 2.0 * std::f32::consts::PI).sin() * 0.6
                        }
                        _ => 0.0,
                    };
                    tex_signal = noise_val * self.tex_env_value * tex_amt * 0.4 * 0.7;
                    // Lowered texture volume by 30%
                }

                let vel_mult = self.midi_velocity * (127.0 / 100.0);
                let pre_drive = (signal + tex_signal) * vel_mult;

                let driven_signal = match drive_model {
                    1 => Self::drive_tape_classic(drive, pre_drive) * 0.89125, // -1dB
                    2 => Self::drive_tape_modern(drive, pre_drive) * 1.412,
                    3 => Self::drive_tube_triode(drive, pre_drive) * 0.917,
                    4 => Self::drive_tube_pentode(drive, pre_drive) * 0.79433, // -2dB
                    5 => {
                        let scaled_drive = drive * 0.59;
                        Self::drive_saturation_digital(scaled_drive, pre_drive) * 0.729
                    }
                    _ => Self::drive_tape_classic(drive, pre_drive) * 0.89125,
                };

                // Ensure all distortion types sound clean when gain is 0% using a dry/wet crossfade mapping
                let drive_wet = drive.sqrt(); // Keep curve musical
                let driven = pre_drive * (1.0 - drive_wet) + driven_signal * drive_wet;

                // ----- Corrosion (Erosion-style modulated delay) -----
                // Returns a stereo (L, R) pair; both are the same when amount == 0.
                let corr_amount = self.params.corrosion_amount.smoothed.next();
                let (corr_l, corr_r) = if corr_amount > 0.0 {
                    let corr_freq_norm = self.params.corrosion_frequency.smoothed.next();
                    let corr_freq = log_scale(corr_freq_norm, 15.0, 22000.0);
                    let corr_width = self.params.corrosion_width.smoothed.next();
                    let corr_blend = self.params.corrosion_noise_blend.smoothed.next();
                    let corr_stereo = self.params.corrosion_stereo.smoothed.next();

                    // Constants matching the Ableton 12.4 spec
                    const BASE_DELAY: f32 = 0.002; // 2 ms
                    const MAX_MOD_DEPTH: f32 = 0.001; // 1 ms max fluctuation

                    // 1. Sine modulators (stereo phase offset)
                    let sine_l = (self.corrosion_sine_phase * std::f32::consts::TAU).sin();
                    let sine_r = ((self.corrosion_sine_phase * std::f32::consts::TAU)
                        + corr_stereo * std::f32::consts::PI)
                        .sin();
                    self.corrosion_sine_phase =
                        (self.corrosion_sine_phase + corr_freq / sample_rate).fract();

                    // 2. Independent white noise for each channel (LCG)
                    self.corrosion_rng = self
                        .corrosion_rng
                        .wrapping_mul(1_664_525)
                        .wrapping_add(1_013_904_223);
                    let raw_noise_l = (self.corrosion_rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
                    self.corrosion_rng = self
                        .corrosion_rng
                        .wrapping_mul(1_664_525)
                        .wrapping_add(1_013_904_223);
                    let raw_noise_r = (self.corrosion_rng as f32 / u32::MAX as f32) * 2.0 - 1.0;

                    // 3. Bandpass-filter noise (2nd-order approximation: LP then HP derived
                    //    from LP; bandwidth controlled by corr_width)
                    let lp_cutoff = (corr_freq * corr_width.max(0.01)).min(sample_rate * 0.499);
                    let hp_cutoff = (corr_freq / corr_width.max(0.01).max(1.0)).max(1.0);
                    let dt = 1.0 / sample_rate;
                    let lp_a = dt / (1.0 / (std::f32::consts::TAU * lp_cutoff) + dt);
                    let hp_a = dt / (1.0 / (std::f32::consts::TAU * hp_cutoff) + dt);

                    // Left channel BP
                    self.corrosion_bp_l[0] += lp_a * (raw_noise_l - self.corrosion_bp_l[0]);
                    let lp_l = self.corrosion_bp_l[0];
                    self.corrosion_bp_l[1] += hp_a * (lp_l - self.corrosion_bp_l[1]);
                    let bp_l = lp_l - self.corrosion_bp_l[1]; // bandpass = LP - LP-of-LP

                    // Right channel BP
                    self.corrosion_bp_r[0] += lp_a * (raw_noise_r - self.corrosion_bp_r[0]);
                    let lp_r = self.corrosion_bp_r[0];
                    self.corrosion_bp_r[1] += hp_a * (lp_r - self.corrosion_bp_r[1]);
                    let bp_r = lp_r - self.corrosion_bp_r[1];

                    // 4. Stereo decorrelation for noise (lerp from mono L → uncorrelated R)
                    let noise_l = bp_l;
                    let noise_r = noise_l + corr_stereo * (bp_r - noise_l);

                    // 5. Noise-blend: crossfade sine <-> bandpassed noise
                    let mod_l = sine_l + corr_blend * (noise_l - sine_l);
                    let mod_r = sine_r + corr_blend * (noise_r - sine_r);

                    // 6. Convert modulation signal to delay time in samples
                    let delay_samples_l =
                        (BASE_DELAY + mod_l * corr_amount * MAX_MOD_DEPTH).max(0.0) * sample_rate;
                    let delay_samples_r =
                        (BASE_DELAY + mod_r * corr_amount * MAX_MOD_DEPTH).max(0.0) * sample_rate;

                    // 7. Write input to delay buffers
                    let buf_len = self.corrosion_buf_l.len();
                    self.corrosion_buf_l[self.corrosion_write] = driven;
                    self.corrosion_buf_r[self.corrosion_write] = driven;

                    // 8. Read back with linear interpolation at the modulated delay time
                    let read_l = Self::corrosion_read(
                        &self.corrosion_buf_l,
                        self.corrosion_write,
                        delay_samples_l,
                    );
                    let read_r = Self::corrosion_read(
                        &self.corrosion_buf_r,
                        self.corrosion_write,
                        delay_samples_r,
                    );

                    self.corrosion_write = (self.corrosion_write + 1) % buf_len;

                    (read_l, read_r)
                } else {
                    (driven, driven)
                };

                out_l = corr_l * self.midi_velocity * 0.9;
                out_r = corr_r * self.midi_velocity * 0.9;

                let abs_l = out_l.abs();
                if abs_l > max_amplitude_in_block_l {
                    max_amplitude_in_block_l = abs_l;
                }

                let abs_r = out_r.abs();
                if abs_r > max_amplitude_in_block_r {
                    max_amplitude_in_block_r = abs_r;
                }

                if self.current_phase != EnvelopePhase::Idle {
                    self.pitch_env_timer += 1.0;
                    self.phase_timer += 1.0;
                }
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
    fn trigger_note(&mut self, velocity: f32) {
        self.midi_velocity = velocity;
        self.current_phase = EnvelopePhase::Attack;
        self.phase_timer = 0.0;
        self.pitch_env_timer = 0.0;
        self.phase = 0.0; // Start at 0-crossing
        self.envelope_value = 0.0;

        // Advance RNG state for analog drift so we always get a new pitch offset
        // regardless of whether texture generation is active.
        self.free_rng_state = self
            .free_rng_state
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);

        // Calculate analog drift for this hit
        self.analog_drift = ((self.free_rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0) * 1.5; // +/- 1.5 Hz

        self.tex_env_phase = EnvelopePhase::Decay;
        self.tex_env_value = 1.0;
        self.wt_phase = 0.0;
        self.static_rng_state = 1337;
        self.tex_release_early = false;
    }

    fn release_note(&mut self) {
        if self.current_phase != EnvelopePhase::Idle && self.current_phase != EnvelopePhase::Release
        {
            self.current_phase = EnvelopePhase::Release;
            self.phase_timer = 0.0;
        }
    }

    // Tape Saturation Type 1: Classic Analog Tape (Soft Knee)
    // Models vintage tape machines with smooth, musical saturation and hysteresis-like behavior
    fn drive_tape_classic(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 12.0;
        let x = signal * gain;

        // Soft saturation curve with tape-like compression
        let saturated = if x.abs() < 0.5 {
            x * (1.0 - 0.15 * x.abs())
        } else {
            let sign = x.signum();
            sign * (0.425 + 0.575 * (1.0 - (-(x.abs() - 0.5) * 3.0).exp()))
        };

        // Apply gentle high-frequency rolloff simulation
        saturated * 0.85
    }

    // Tape Saturation Type 2: Modern High-Bias Tape (Asymmetric)
    // Models modern tape with asymmetric clipping and warmth
    fn drive_tape_modern(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 10.0;
        let x = signal * gain;

        // Asymmetric tape saturation (positive/negative behave differently)
        let saturated = if x >= 0.0 {
            // Positive: harder saturation
            x / (1.0 + x.abs().powf(1.4))
        } else {
            // Negative: softer saturation
            x / (1.0 + (x.abs() * 0.85).powf(1.2))
        };

        saturated * 0.9
    }

    // Tube Saturation Type 1: Triode Tube (Warm, Musical)
    // Models classic triode tube stages with even/odd harmonics
    fn drive_tube_triode(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 15.0;
        let x = signal * gain;

        // Triode-style transfer curve with crossover distortion
        let saturated = if x.abs() < 0.1 {
            x * 0.95 // Slight deadzone for tube character
        } else {
            // Soft tube saturation using tanh for guaranteed bounding
            let sign = x.signum();
            let abs_x = x.abs();
            let mapped = (abs_x - 0.1) * 1.5;
            sign * (0.095 + mapped.tanh() * 0.905)
        };

        // Output scaling with tube warmth
        saturated * 0.75
    }

    // Tube Saturation Type 2: Pentode/Power Tube (Aggressive)
    // Models power tube stages with harder clipping and compression
    fn drive_tube_pentode(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 18.0;
        let x = signal * gain;

        // Power tube saturation with grid clipping simulation
        let saturated = if x.abs() < 0.3 {
            x
        } else {
            // Bounded hard limit transition using atan for stability
            let sign = x.signum();
            let abs_x = x.abs();
            let mapped = (abs_x - 0.3) * 2.0;
            sign * (0.3 + mapped.atan() * (0.7 / std::f32::consts::FRAC_PI_2))
        };

        saturated * 0.7
    }

    // Digital Saturation: Modern Clipper with Oversampling Simulation
    // Clean, transparent saturation with minimal aliasing artifacts
    fn drive_saturation_digital(drive: f32, signal: f32) -> f32 {
        let gain = 1.0 + drive * 20.0;
        let x = signal * gain;

        // Quintic polynomial approximation (C2 continuous)
        let saturated = if x.abs() <= 1.0 {
            x * (1.5 - 0.5 * x.abs().powf(2.0))
        } else {
            x.signum()
        };

        // Apply soft knee at threshold
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

    /// Fractional delay buffer reader using linear interpolation.
    /// `buf` is a circular delay buffer, `write_pos` is the current write head,
    /// `delay_samples` is the number of samples to look back (may be fractional).
    fn corrosion_read(buf: &[f32], write_pos: usize, delay_samples: f32) -> f32 {
        let len = buf.len();
        let delay_i = delay_samples as usize;
        let frac = delay_samples - delay_i as f32;

        // Clamp so we never exceed the buffer length
        let delay_i = delay_i.min(len - 1);

        // Read head (going backward from the write position)
        let idx0 = (write_pos + len - delay_i) % len;
        let idx1 = (write_pos + len - delay_i - 1) % len;

        // Linear interpolation between the two adjacent samples
        buf[idx0] + frac * (buf[idx1] - buf[idx0])
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

    // Update left meter
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

    // Update right meter
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
