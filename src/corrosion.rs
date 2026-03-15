/*
 * Copyright (C) 2026 Marinus Burger
 */

//! Corrosion effect: an LFO/noise-modulated stereo delay that creates subtle
//! pitch and time-smearing artefacts reminiscent of analogue tape wow/flutter
//! and machine degradation.

use std::cell::RefCell;
use std::sync::Arc;

use crate::params::KickParams;
use crate::params::log_scale;

// ── Internal state ──────────────────────────────────────────────────────────────

pub struct CorrosionState {
    pub buf_l: Vec<f32>,
    pub buf_r: Vec<f32>,
    pub write: usize,
    pub sine_phase: f32,
    pub bp_l: [f32; 2],
    pub bp_r: [f32; 2],
    pub rng: u32,
}

// ── Internal helper ─────────────────────────────────────────────────────────────

/// Read a fractional sample from a circular delay buffer at `delay_samples`
/// behind the current `write_pos`.
#[inline(always)]
pub fn read_delayed(buf: &[f32], write_pos: usize, delay_samples: f32) -> f32 {
    let len = buf.len();
    let delay_i = delay_samples as usize;
    let frac = delay_samples - delay_i as f32;
    let delay_i = delay_i.min(len - 1);
    let idx0 = (write_pos + len - delay_i) % len;
    let idx1 = (write_pos + len - delay_i - 1) % len;
    buf[idx0] + frac * (buf[idx1] - buf[idx0])
}

// ── Main processing function ────────────────────────────────────────────────────

/// Apply the corrosion effect to a single mono `driven` sample and return a
/// stereo pair `(left, right)`.
///
/// When `corr_amount` is 0.0 the function early-exits and returns
/// `(driven, driven)` without touching the delay buffers.
pub fn apply(
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
        let read_l = read_delayed(&state.buf_l, write_pos, delay_samples_l);
        let read_r = read_delayed(&state.buf_r, write_pos, delay_samples_r);

        (read_l, read_r)
    } else {
        (driven, driven)
    }
}
