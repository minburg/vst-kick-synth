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

    /// Drive / Distortion amount
    #[id = "drive"]
    pub drive: FloatParam,

    /// Manual Trigger Button
    #[id = "trigger"]
    pub trigger: BoolParam,

    /// Trigger Logic for the UI (consumed by audio thread)
    pub gui_trigger: AtomicBool,
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),

            gui_trigger: AtomicBool::new(false),

            tune: FloatParam::new(
                "Tune",
                50.0,
                FloatRange::Linear {
                    min: 30.0,
                    max: 150.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} hz", value))),

            sweep: FloatParam::new(
                "Sweep",
                200.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1000.0,
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            pitch_decay: FloatParam::new(
                "Pitch Decay",
                50.0,
                FloatRange::Linear {
                    min: 5.0,
                    max: 500.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} ms", value))),

            attack: FloatParam::new(
                "Attack",
                1.0,
                FloatRange::Linear {
                    min: 0.1,
                    max: 100.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} ms", value))),

            decay: FloatParam::new(
                "Decay",
                200.0,
                FloatRange::Linear {
                    min: 10.0,
                    max: 1000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} ms", value))),

            sustain: FloatParam::new("Sustain", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(1)),

            release: FloatParam::new(
                "Release",
                100.0,
                FloatRange::Linear {
                    min: 10.0,
                    max: 1000.0,
                },
            )
            .with_value_to_string(Arc::new(move |value| format!("{:.1} ms", value))),

            drive: FloatParam::new("Drive", 0.1, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(1)),

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

    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
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
            let trigger_param_val = self.params.trigger.value();

            if gui_triggered || (trigger_param_val && !self.was_trigger_on) {
                self.trigger_note();
            }
            self.was_trigger_on = trigger_param_val;

            // 2. MIDI Handle
            while let Some(event) = next_event {
                if event.timing() > sample_idx as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { velocity, .. } => {
                        if velocity > 0.0 {
                            self.trigger_note();
                        }
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
                        self.envelope_value -= sustain_lvl / release_samples;
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

                let driven_signal = (signal * (1.0 + drive * 8.0)).tanh();
                output = driven_signal;

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
    fn trigger_note(&mut self) {
        self.current_phase = EnvelopePhase::Attack;
        self.phase_timer = 0.0;
        self.pitch_env_timer = 0.0;
        self.phase = 0.0; // Start at 0-crossing
        self.envelope_value = 0.0;
    }
}

impl Vst3Plugin for KickSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"BrgrKickSynthV01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(KickSynth);
