/*
 * Copyright (C) 2026 Marinus Burger
 */

//! Plugin entry point and top-level orchestration.
//!
//! This file is intentionally small — it only wires together the specialised
//! sub-modules and implements the nih-plug `Plugin` trait.  All domain logic
//! lives in dedicated modules:
//!
//! | Module          | Responsibility                                    |
//! |-----------------|---------------------------------------------------|
//! | `params`        | `KickParams`, `NamModel`, parameter definitions   |
//! | `voice`         | `VoiceState`, oscillators, texture, envelope ADSR |
//! | `drive`         | Saturation / distortion algorithms                |
//! | `corrosion`     | Wow/flutter-style stereo delay effect             |
//! | `filter`        | Moog Ladder + TPT SVF filter engine               |
//! | `presets`       | Factory preset data and serialisation             |
//! | `editor`        | Vizia GUI (all UI panels and widgets)             |
//! | `nam`           | Neural Amp Modeller integration (feature = "nam") |

use nih_plug::prelude::AtomicF32;
use nih_plug::prelude::*;
use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;

// ── Modules ─────────────────────────────────────────────────────────────────────

mod corrosion;
mod drive;
mod editor;
mod filter;
mod params;
mod presets;
mod voice;
#[cfg(feature = "nam")]
mod nam;

// ── Re-exports (public surface used by editor / presets) ────────────────────────

pub use filter::{FilterPosition, FilterType};
pub use params::{KickParams, NamModel};

use corrosion::CorrosionState;
use filter::FilterEngine;
use voice::{EnvelopePhase, SmoothedParams, VoiceState};

// ── NAM model selection ─────────────────────────────────────────────────────────

/// All NAM models are normalized to this integrated loudness (LUFS) after loading.
/// -18 LUFS gives headroom above broadcast (-23) while still being loud enough for a kick plugin.
#[cfg(feature = "nam")]
const NAM_TARGET_LUFS: f32 = -18.0;

/// Extracts `metadata.loudness` (LUFS) from a .nam JSON string.
/// Returns `None` if the field is absent or cannot be parsed.
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
#[cfg(feature = "nam")]
fn model_pre_input_db(model: NamModel) -> f32 {
    match model {
        NamModel::PhilipsEL3541D => 0.0,
        NamModel::CultureVulture => 0.0,
        NamModel::JH24           => 15.0,
    }
}

/// Per-model empirical trim (dB) added on top of the loudness-based calibration.
/// Target: –12 dBFS peak at 0 dB input gain.
#[cfg(feature = "nam")]
fn model_reference_trim_db(model: NamModel) -> f32 {
    match model {
        NamModel::PhilipsEL3541D => -5.0,
        NamModel::CultureVulture => -2.0,
        NamModel::JH24           =>  3.0,
    }
}

// ── KickSynth ────────────────────────────────────────────────────────────────────

pub struct KickSynth {
    params: Arc<KickParams>,
    sample_rate: f32,

    voice: VoiceState,
    releasing_voice: Option<VoiceState>,

    #[cfg(debug_assertions)]
    debug_phase: f32,
    #[cfg(debug_assertions)]
    debug_release_timer: f32,

    // Texture / oscillator tables
    wavetable: Vec<f32>,
    sampled_noise: Vec<f32>,
    free_rng_state: Cell<u32>,

    // Corrosion state (interior mutability because apply_corrosion borrows only the corrosion)
    corrosion: RefCell<CorrosionState>,

    meter_decay_per_sample: f32,
    peak_meter_l: Arc<AtomicF32>,
    peak_meter_r: Arc<AtomicF32>,

    // Trigger state
    was_trigger_on: bool,
    active_midi_note: Option<u8>,

    #[cfg(feature = "nam")]
    nam_synth: nam::NamSynth,
    #[cfg(feature = "nam")]
    current_nam_model: Option<NamModel>,
    /// Internal per-model output gain normalization (computed at load time, NOT user-controlled)
    #[cfg(feature = "nam")]
    nam_calibration_gain: f32,
    /// Internal per-model input pre-scale applied before the NAM block.
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
            value += (white - value) * 0.1;
            sampled_noise[i] = value;
        }
        let max_noise = sampled_noise.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if max_noise > 0.0 {
            for sample in sampled_noise.iter_mut() {
                *sample /= max_noise;
            }
        }

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
            peak_meter_l: Arc::new(AtomicF32::new(0.0)),
            peak_meter_r: Arc::new(AtomicF32::new(0.0)),

            was_trigger_on: false,
            active_midi_note: None,

            #[cfg(feature = "nam")]
            nam_synth: nam::NamSynth::new(44100.0, 2048),
            #[cfg(feature = "nam")]
            current_nam_model: None,
            #[cfg(feature = "nam")]
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

// ── Plugin impl ──────────────────────────────────────────────────────────────────

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

        let corrosion_buf_size =
            ((_buffer_config.sample_rate * 0.004) as usize + 4).next_power_of_two();
        let mut corrosion_state = self.corrosion.borrow_mut();
        corrosion_state.buf_l = vec![0.0; corrosion_buf_size];
        corrosion_state.buf_r = vec![0.0; corrosion_buf_size];
        corrosion_state.write = 0;

        let release_db_per_second = 160.0;
        self.meter_decay_per_sample = f32::powf(
            10.0,
            -release_db_per_second / (20.0 * _buffer_config.sample_rate),
        );

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
                let content = match selected_model {
                    NamModel::PhilipsEL3541D => params::NAM_MODEL_PHILIPS,
                    NamModel::CultureVulture => params::NAM_MODEL_CULTURE_VULTURE,
                    NamModel::JH24 => params::NAM_MODEL_JH24,
                };

                match self.nam_synth.load_model_content(content) {
                    Ok(_) => {
                        self.params.nam_is_loaded.store(true, Ordering::SeqCst);
                        if let Some(mut text) = self.params.nam_status_text.try_write() {
                            *text = String::from("NAM Loaded");
                        }
                        let loudness = parse_nam_loudness(content).unwrap_or(NAM_TARGET_LUFS);
                        let loudness_offset_db = NAM_TARGET_LUFS - loudness;
                        let trim_db = model_reference_trim_db(selected_model);
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

        if trigger_count > 0 {
            self.trigger_note(1.0, self.active_midi_note.unwrap_or(36).saturating_sub(24));
        }
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

        if filter_type != self.last_filter_type || filter_position != self.last_filter_position {
            self.filter_engine.clear();
            self.last_filter_type = filter_type;
            self.last_filter_position = filter_position;
        }

        let mut output_gains: Vec<f32> = Vec::with_capacity(num_samples);
        let nam_active_value = self.params.nam_active.value();

        for sample_idx in 0..num_samples {
            #[cfg(debug_assertions)]
            {
                self.debug_phase += 1.0 / self.sample_rate;
                if self.debug_phase >= 0.5 {
                    self.debug_phase -= 0.5;
                    self.trigger_note(0.8, 12);
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
                        } else if self.active_midi_note == Some(note) {
                            self.release_note();
                            self.active_midi_note = None;
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

            let smoothed = SmoothedParams {
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
                nam_input_gain: self.params.nam_input_gain.smoothed.next(),
                output_gain: self.params.output_gain.smoothed.next(),
                bass_synth_mode: self.params.bass_synth_mode.value(),
            };

            let mut mono_sample = voice::compute_voice_sample(
                &mut self.voice,
                &smoothed,
                self.sample_rate,
                &self.free_rng_state,
                &self.wavetable,
                &self.sampled_noise,
            );

            if let Some(releasing_voice) = &mut self.releasing_voice {
                mono_sample += voice::compute_voice_sample(
                    releasing_voice,
                    &smoothed,
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
                util::db_to_gain_fast(smoothed.nam_input_gain) * self.nam_pre_input_scale
            } else {
                1.0
            };
            self.mono_buffer[sample_idx] = mono_sample * input_gain_amp;

            output_gains.push(util::db_to_gain_fast(smoothed.output_gain));
        }

        // Filter — PreNam position
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
            self.nam_output_buffer[0..num_samples]
                .copy_from_slice(&self.mono_buffer[0..num_samples]);
        }
        #[cfg(not(feature = "nam"))]
        self.nam_output_buffer[0..num_samples].copy_from_slice(&self.mono_buffer[0..num_samples]);

        // Filter — PostNam position
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

        // 4. Post-NAM: Stereoize and write to output
        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {
            let calibration_gain = if nam_active_value {
                #[cfg(feature = "nam")]
                { self.nam_calibration_gain }
                #[cfg(not(feature = "nam"))]
                { 1.0 }
            } else {
                1.0
            };

            let master_gain = output_gains[sample_idx];
            let driven = self.nam_output_buffer[sample_idx] * calibration_gain * master_gain;

            let (mut out_l, mut out_r) = corrosion::apply(
                self.sample_rate,
                driven,
                self.params.corrosion_amount.value(),
                &self.corrosion,
                &self.params,
            );

            // Filter — PostAll position
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

// ── Voice helpers (on KickSynth) ─────────────────────────────────────────────────

impl KickSynth {
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

        self.voice.analog_drift =
            ((next_rng as f32 / u32::MAX as f32) * 2.0 - 1.0) * 1.5;

        self.voice.tex_env_phase = EnvelopePhase::Decay;
        self.voice.tex_env_value = 1.0;

        self.filter_engine.envelope.mode = if self.params.filter_env_trigger.value() {
            filter::FilterEnvMode::Trigger
        } else {
            filter::FilterEnvMode::Gate
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
        if !self.params.filter_env_trigger.value() {
            self.filter_engine.release();
        }
    }
}

// ── Peak meter helper ────────────────────────────────────────────────────────────

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

// ── VST3 export ──────────────────────────────────────────────────────────────────

impl Vst3Plugin for KickSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"BrgrKickSynthV01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(KickSynth);
