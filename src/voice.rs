/*
 * Copyright (C) 2026 Marinus Burger
 */

//! Voice state, envelope phases, smoothed parameter snapshot, and the
//! per-sample synthesis function.
//!
//! Everything here runs on the audio thread; no allocations, no locks.

use std::cell::Cell;

// ── Envelope phase ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopePhase {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

// ── VoiceState ──────────────────────────────────────────────────────────────────

/// All per-voice state that must survive from one audio sample to the next.
#[derive(Clone)]
pub struct VoiceState {
    pub phase: f32,
    pub envelope_value: f32,
    pub current_phase: EnvelopePhase,
    pub phase_timer: f32,
    pub release_coeff: f32,
    pub pitch_env_timer: f32,
    pub tex_env_value: f32,
    pub tex_env_phase: EnvelopePhase,
    pub wt_phase: f32,
    pub tex_filter_state: f32,
    pub tex_filter_state_2: f32,
    pub static_rng_state: u32,
    pub midi_velocity: f32,
    pub analog_drift: f32,
    pub fast_release: bool,
    pub midi_note: u8,
    /// Second oscillator phase used only in Sub mode (waveform 5).
    /// Advances at half the fundamental frequency so it never aliases when
    /// `voice.phase` wraps — it needs its own independent counter.
    pub waveform_phase2: f32,
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

// ── SmoothedParams ──────────────────────────────────────────────────────────────

/// Snapshot of all smoothed/block-rate parameters consumed by `compute_voice_sample`.
/// Built once per audio sample inside the processing loop.
pub struct SmoothedParams {
    pub tune: f32,
    pub waveform: i32,
    pub sweep: f32,
    pub pitch_decay: f32,
    pub drive: f32,
    pub drive_model: i32,
    pub tex_amt: f32,
    pub tex_decay: f32,
    pub tex_variation: f32,
    pub analog_variation: f32,
    pub tex_type: i32,
    pub tex_tone: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub nam_input_gain: f32,
    pub output_gain: f32,
    pub bass_synth_mode: bool,
}

// ── Per-sample synthesis ────────────────────────────────────────────────────────

/// Advance `voice` by one sample and return the raw mono output.
///
/// This is a pure function — it only accesses the voice state and read-only
/// shared data (`wavetable`, `sampled_noise`, `free_rng_state`).
pub fn compute_voice_sample(
    voice: &mut VoiceState,
    params: &SmoothedParams,
    sample_rate: f32,
    free_rng_state: &Cell<u32>,
    wavetable: &[f32],
    sampled_noise: &[f32],
) -> f32 {
    if voice.current_phase != EnvelopePhase::Idle || voice.tex_env_phase != EnvelopePhase::Idle
    {
        // ── Amplitude ADSR ───────────────────────────────────────────────────
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

        // ── Pitch Envelope ───────────────────────────────────────────────────
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

        // ── Waveform modes ────────────────────────────────────────────────────
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

        // ── Texture Layer ─────────────────────────────────────────────────────
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
                        0.5 - (0.5 - params.tex_tone) * 1.5
                    } else {
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
                    let freq = 1000.0 * 4.0_f32.powf(2.0_f32 * params.tex_tone);
                    let mut mod_freq = freq * 0.5 * params.tex_variation;
                    if params.tex_variation <= 0.0 {
                        mod_freq = freq * 0.5;
                    }

                    let drift = (free_val - 0.5) * 200.0 * params.tex_variation;
                    voice.tex_filter_state_2 =
                        (voice.tex_filter_state_2 + (mod_freq + drift) / sample_rate).fract();
                    let mo =
                        (voice.tex_filter_state_2 * 2.0 * std::f32::consts::PI).sin();

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
            1 => crate::drive::tape_classic(params.drive, pre_drive) * 0.89125,
            2 => crate::drive::tape_modern(params.drive, pre_drive) * 1.412,
            3 => crate::drive::tube_triode(params.drive, pre_drive) * 0.917,
            4 => crate::drive::tube_pentode(params.drive, pre_drive) * 0.79433,
            5 => {
                let scaled_drive = params.drive * 0.59;
                crate::drive::saturation_digital(scaled_drive, pre_drive) * 0.729
            }
            _ => crate::drive::tape_classic(params.drive, pre_drive) * 0.89125,
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
