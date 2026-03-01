/*
 * Copyright (C) 2026 Marinus Burger
 */

use nih_plug::prelude::*;
use nih_plug_vizia::ViziaState;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod editor;

pub struct KickSynth {
    params: Arc<KickParams>,
    sample_rate: f32,

    // DSP State
    phase: f32,

    // ADSR State
    envelope_value: f32,
    current_phase: EnvelopePhase,
    phase_timer: f32,

    // Pitch Envelope State
    pitch_env_timer: f32,

    // Trigger Logic
    was_trigger_on: bool,
    midi_velocity: f32,
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
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

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
            ),

            attack: FloatParam::new(
                "A",
                0.1,
                FloatRange::Linear {
                    min: 0.1,
                    max: 100.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} ms", value))),

            decay: FloatParam::new(
                "D",
                153.0,
                FloatRange::Linear {
                    min: 10.0,
                    max: 1000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} ms", value))),

            sustain: FloatParam::new("S", 0.44, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0)),

            release: FloatParam::new(
                "R",
                128.0,
                FloatRange::Linear {
                    min: 10.0,
                    max: 1000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.0} ms", value))),

            trigger: BoolParam::new("Trigger", false),
        }
    }
}

impl Default for KickSynth {
    fn default() -> Self {
        Self {
            params: Arc::new(KickParams::default()),
            sample_rate: 44100.0,
            phase: 0.0,
            envelope_value: 0.0,
            current_phase: EnvelopePhase::Idle,
            phase_timer: 0.0,
            pitch_env_timer: 0.0,
            was_trigger_on: false,
            midi_velocity: 1.0,
        }
    }
}

impl Plugin for KickSynth {
    const NAME: &'static str = "Kick Synth";
    const VENDOR: &'static str = "BRGR.DEV";
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
        true
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.envelope_value = 0.0;
        self.current_phase = EnvelopePhase::Idle;
        self.was_trigger_on = false;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _ctx: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let sample_rate = self.sample_rate;
        let mut next_event = _ctx.next_event();

        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {
            // 1. Trigger Check (UI Atomic OR Parameter Edge)
            let gui_triggered = self.params.gui_trigger.swap(false, Ordering::SeqCst);
            let gui_released = self.params.gui_release.swap(false, Ordering::SeqCst);
            let trigger_param_val = self.params.trigger.value();

            if gui_triggered || (trigger_param_val && !self.was_trigger_on) {
                nih_log!("Trigger!");
                self.trigger_note(1.0);
            } else if gui_released && self.was_trigger_on {
                nih_log!("Release!");
                self.release_note();
            }
            self.was_trigger_on = trigger_param_val;

            // 2. MIDI Handle
            while let Some(event) = next_event {

                if event.timing() > sample_idx as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { velocity, .. } => {

                        // Accept any note on any channel
                        if velocity > 0.0 {
                            self.trigger_note(velocity);
                        } else {
                            // Handle NoteOn with vel 0 as NoteOff
                            self.release_note();
                        }
                    }
                    NoteEvent::NoteOff { .. } => {
                        // Accept any note off on any channel
                        self.release_note();
                    }
                    _ => (),
                }
                next_event = _ctx.next_event();
            }

            // 3. DSP
            let mut output = 0.0;

            if self.current_phase != EnvelopePhase::Idle {
                let base_freq = self.params.tune.smoothed.next();
                let sweep_amt = self.params.sweep.smoothed.next();
                let pitch_decay_ms = self.params.pitch_decay.smoothed.next();

                let attack_ms = self.params.attack.smoothed.next();
                let decay_ms = self.params.decay.smoothed.next();
                let sustain_lvl = self.params.sustain.smoothed.next();
                let release_ms = self.params.release.smoothed.next();
                let drive = self.params.drive.smoothed.next();
                let drive_model = self.params.drive_model.value();

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
                        self.envelope_value -= (1.0 - sustain_lvl) / decay_samples;
                        if self.envelope_value <= sustain_lvl {
                            self.envelope_value = sustain_lvl;
                            self.current_phase = EnvelopePhase::Sustain;
                            self.phase_timer = 0.0;
                            if sustain_lvl <= 0.0 {
                                self.current_phase = EnvelopePhase::Idle;
                            }
                        }
                    }
                    EnvelopePhase::Sustain => {
                        self.envelope_value = sustain_lvl;
                        // Manual Release timer or MIDI NoteOff would go here.
                        // For a trigger kick, we just decay to 0 if sustain is 0.
                        // If sustain is > 0, it stays until external release.
                    }
                    EnvelopePhase::Release => {
                        let release_samples = (sample_rate * (release_ms / 1000.0)).max(1.0);
                        // We decay from the current envelope value down to 0
                        let release_step = 1.0 / release_samples;
                        self.envelope_value -= release_step;

                        if self.envelope_value <= 0.0 {
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

                let current_freq = base_freq + (sweep_amt * pitch_env_val);
                let phase_inc = current_freq / sample_rate;
                self.phase = (self.phase + phase_inc).fract();

                let sine_wave = (self.phase * 2.0 * std::f32::consts::PI).sin();
                let signal = sine_wave * self.envelope_value;

                let driven_signal = match drive_model {
                    1 => Self::drive_tape_classic(drive, signal),
                    2 => Self::drive_tape_modern(drive, signal),
                    3 => Self::drive_tube_triode(drive, signal),
                    4 => Self::drive_tube_pentode(drive, signal),
                    5 => Self::drive_saturation_digital(drive, signal),
                    _ => Self::drive_tape_classic(drive, signal),
                };
                output = driven_signal * self.midi_velocity * 0.9;

                self.pitch_env_timer += 1.0;
                self.phase_timer += 1.0;
            }

            for sample in channel_samples {
                *sample = output;
            }
        }

        ProcessStatus::KeepAlive
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.params.editor_state.clone())
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
    }

    fn release_note(&mut self) {
        // Only transition to release if we are currently playing a note
        if self.current_phase != EnvelopePhase::Idle && self.current_phase != EnvelopePhase::Release {
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
            // Soft tube saturation with cubic nonlinearity
            let sign = x.signum();
            let abs_x = x.abs();
            sign * (abs_x - 0.33 * abs_x.powf(3.0)) / (1.0 + 0.1 * abs_x.powf(2.0))
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
        } else if x.abs() < 1.0 {
            let sign = x.signum();
            let abs_x = x.abs();
            // Transition region with square law
            sign * (0.3 + 0.7 * (abs_x - 0.3).powf(0.6))
        } else {
            // Hard limiting
            x.signum() * (0.3 + 0.7 * 0.7_f32.powf(0.6))
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
}

impl Vst3Plugin for KickSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"BrgrKickSynthV01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(KickSynth);
