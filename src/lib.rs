/*
 * Copyright (C) 2026 Marinus Burger
 */

use nih_plug::prelude::*;
use nih_plug_vizia::ViziaState;
use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use nih_plug::prelude::AtomicF32;
use parking_lot::RwLock;


mod editor;
mod nam;


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

    nam_synth: nam::NamSynth,
    current_nam_path: String,

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

    #[id = "nam_input_gain"]
    pub nam_input_gain: FloatParam,

    #[id = "nam_output_gain"]
    pub nam_output_gain: FloatParam,

    #[persist = "nam_model_path"]
    pub nam_model_path: Arc<RwLock<String>>,
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

            nam_input_gain: FloatParam::new(
                "NAM Input Gain",
                0.0,
                FloatRange::Linear { min: -18.0, max: 18.0 },
            )
            .with_unit("dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            nam_output_gain: FloatParam::new(
                "NAM Output Gain",
                0.0,
                FloatRange::Linear { min: -18.0, max: 18.0 },
            )
            .with_unit("dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            nam_model_path: Arc::new(RwLock::new(String::from("src/resource/nam/Philips_EL3541D.nam"))),
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

            nam_synth: nam::NamSynth::new(44100.0, 2048),
            current_nam_path: String::new(),

            mono_buffer: Vec::with_capacity(2048),
            nam_output_buffer: Vec::with_capacity(2048),
        }
    }
}

struct SmoothedParams {
    tune: f32,
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
    nam_output_gain: f32,
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
        self.mono_buffer.resize(_buffer_config.max_buffer_size as usize, 0.0);
        self.nam_output_buffer.resize(_buffer_config.max_buffer_size as usize, 0.0);
        self.nam_synth.update_settings(_buffer_config.sample_rate, _buffer_config.max_buffer_size as i32);

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

        // 0. Check for model path updates
        let model_path = self.params.nam_model_path.read().clone();
        if model_path != self.current_nam_path {
            self.nam_synth.load_model(&model_path);
            self.current_nam_path = model_path;
        }

        // 1. Generate mono synth signal for the Block
        let num_samples = buffer.samples();
        for sample_idx in 0..num_samples {
            #[cfg(debug_assertions)]
            {
                self.debug_phase += 1.0 / self.sample_rate;
                if self.debug_phase >= 0.5 {
                    self.debug_phase -= 0.5;
                    self.trigger_note(0.8);
                    self.debug_release_timer = 0.2 * self.sample_rate;
                }

                if self.debug_release_timer > 0.0 {
                    self.debug_release_timer -= 1.0;
                    if self.debug_release_timer <= 0.0 {
                        self.release_note();
                    }
                }
            }

            // Trigger Check
            let gui_triggered = self.params.gui_trigger.swap(false, Ordering::SeqCst);
            let gui_released = self.params.gui_release.swap(false, Ordering::SeqCst);
            let trigger_param_val = self.params.trigger.value();

            if gui_triggered || (trigger_param_val && !self.was_trigger_on) {
                self.trigger_note(1.0);
            } else if gui_released && self.was_trigger_on {
                self.release_note();
            }
            self.was_trigger_on = trigger_param_val;

            // MIDI Handle
            while let Some(event) = next_event {
                if event.timing() > sample_idx as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { velocity, note, .. } => {
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
                nam_output_gain: self.params.nam_output_gain.smoothed.next(),
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
                if releasing_voice.current_phase == EnvelopePhase::Idle && releasing_voice.tex_env_phase == EnvelopePhase::Idle {
                    self.releasing_voice = None;
                }
            }
            
            let input_gain_amp = util::db_to_gain_fast(params.nam_input_gain);
            self.mono_buffer[sample_idx] = mono_sample * input_gain_amp;
        }

        // 2. Apply NAM Block
        self.nam_synth.process_block(
            &self.mono_buffer[0..num_samples],
            &mut self.nam_output_buffer[0..num_samples],
        );

        let output_gain_amp = util::db_to_gain_fast(self.params.nam_output_gain.smoothed.next());

        // 3. Post-NAM: Stereoize and write to output
        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {
            let driven = self.nam_output_buffer[sample_idx] * output_gain_amp;
            
            // Note: We use the current smoothed value of corrosion_amount for the second pass.
            // In a better implementation, we'd buffer the smoothed values too, but this is usually fine for parameters.
            let (out_l, out_r) = Self::apply_corrosion(
                self.sample_rate,
                driven,
                self.params.corrosion_amount.value(),
                &self.corrosion,
                &self.params,
            );

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
        if voice.current_phase != EnvelopePhase::Idle || voice.tex_env_phase != EnvelopePhase::Idle {
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

            let current_freq =
                params.tune + (voice.analog_drift * params.analog_variation) + (params.sweep * pitch_env_val);
            let phase_inc = current_freq / sample_rate;
            voice.phase = (voice.phase + phase_inc).fract();

            let sine_wave = (voice.phase * 2.0 * std::f32::consts::PI).sin();
            let signal = sine_wave * voice.envelope_value;

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
                free_rng_state
                    .set(current_free_rng.wrapping_mul(1664525).wrapping_add(1013904223));

                let static_val = (voice.static_rng_state as f32) / (u32::MAX as f32);
                let free_val = (current_free_rng as f32) / (u32::MAX as f32);

                let static_sym = static_val * 2.0 - 1.0;
                let free_sym = free_val * 2.0 - 1.0;

                let noise_val = match params.tex_type {
                    1 => {
                        let threshold = 0.999 - (params.tex_tone * 0.1);
                        let static_dust = if static_val > threshold { static_sym } else { 0.0 };
                        let free_dust = if free_val > threshold { free_sym } else { 0.0 };
                        let raw_dust =
                            static_dust * (1.0 - params.tex_variation) + free_dust * params.tex_variation;

                        let cutoff = 8000.0 * (1.0 - params.tex_tone * 0.9);
                        let dt = 1.0 / sample_rate;
                        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
                        let alpha = dt / (rc + dt);

                        voice.tex_filter_state += alpha * (raw_dust - voice.tex_filter_state);
                        voice.tex_filter_state * 3.0
                    }
                    2 => {
                        let cutoff = 200.0 * 50.0_f32.powf(params.tex_tone);
                        let dt = 1.0 / sample_rate;
                        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
                        let alpha = dt / (rc + dt);

                        let eq_pwr_static = (1.0 - params.tex_variation).sqrt();
                        let eq_pwr_free = params.tex_variation.sqrt();
                        let combined_noise =
                            static_sym * eq_pwr_static + free_sym * eq_pwr_free;

                        voice.tex_filter_state +=
                            alpha * (combined_noise - voice.tex_filter_state);
                        let shaped = voice.tex_filter_state
                            * voice.tex_filter_state
                            * voice.tex_filter_state
                            * 10.0;
                        shaped.clamp(-1.0, 1.0)
                    }
                    3 => {
                        let mut t = voice.wt_phase * (sampled_noise.len() as f32);
                        let playback_speed = 0.5 + params.tex_tone;

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
                        let base_freq = 20.0 * 50.0_f32.powf(params.tex_tone);
                        let eq_pwr_static = (1.0 - params.tex_variation).sqrt();
                        let eq_pwr_free = params.tex_variation.sqrt();
                        let combined_noise =
                            static_sym * eq_pwr_static + free_sym * eq_pwr_free;

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
                        let eq_pwr_static = (1.0 - params.tex_variation).sqrt();
                        let eq_pwr_free = params.tex_variation.sqrt();
                        let hiss_noise = static_sym * eq_pwr_static + free_sym * eq_pwr_free;

                        let mix_val = static_val * (1.0 - params.tex_variation) + free_val * params.tex_variation;
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
                        let freq = 1000.0 * 4.0_f32.powf(params.tex_tone);
                        let mut mod_freq = freq * 0.5 * params.tex_variation;
                        if params.tex_variation <= 0.0 {
                            mod_freq = freq * 0.5;
                        }

                        let drift = (free_val - 0.5) * 200.0 * params.tex_variation;
                        voice.tex_filter_state_2 = (voice.tex_filter_state_2
                            + (mod_freq + drift) / sample_rate)
                            .fract();
                        let mo = (voice.tex_filter_state_2 * 2.0 * std::f32::consts::PI).sin();

                        voice.wt_phase =
                            (voice.wt_phase + (freq + mo * 1000.0) / sample_rate).fract();
                        (voice.wt_phase * 2.0 * std::f32::consts::PI).sin() * 0.6
                    }
                    _ => 0.0,
                };
                tex_signal = noise_val * voice.tex_env_value * params.tex_amt * 0.4 * 0.7;
            }

            let vel_mult = voice.midi_velocity * (127.0 / 100.0);
            let pre_drive = (signal + tex_signal) * vel_mult;

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

            let output_val = driven * voice.midi_velocity * 0.9;

            if voice.current_phase != EnvelopePhase::Idle {
                voice.pitch_env_timer += 1.0;
                voice.phase_timer += 1.0;
            }
            output_val
        } else {
            0.0
        }
    }

    fn trigger_note(&mut self, velocity: f32) {
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
        self.voice.midi_velocity = velocity;
        self.voice.current_phase = EnvelopePhase::Attack;

        let current_rng = self.free_rng_state.get();
        let next_rng = current_rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.free_rng_state.set(next_rng);

        self.voice.analog_drift = ((next_rng as f32 / u32::MAX as f32) * 2.0 - 1.0) * 1.5;

        self.voice.tex_env_phase = EnvelopePhase::Decay;
        self.voice.tex_env_value = 1.0;
    }

    fn release_note(&mut self) {
        if self.voice.current_phase != EnvelopePhase::Idle && self.voice.current_phase != EnvelopePhase::Release
        {
            self.voice.current_phase = EnvelopePhase::Release;
            self.voice.phase_timer = 0.0;
            self.voice.fast_release = false;
        }
        if self.voice.tex_env_phase != EnvelopePhase::Idle && self.voice.tex_env_phase != EnvelopePhase::Release {
            self.voice.tex_env_phase = EnvelopePhase::Release;
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
                let sine_r =
                    ((state.sine_phase * std::f32::consts::TAU) + corr_stereo * std::f32::consts::PI)
                        .sin();
                state.sine_phase = (state.sine_phase + corr_freq / sample_rate).fract();

                state.rng = state.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let raw_noise_l = (state.rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
                state.rng = state.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
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
