//! Filter engine v2 — FabFilter Volcano 3 inspired filter styles.
//!
//! This module provides the same filter topologies as `filter.rs` (Moog Ladder
//! and Cytomic TPT SVF), but replaces the single fixed nonlinearity with four
//! distinct saturation characters, inspired by the style selector in FabFilter
//! Volcano 3:
//!
//! | Style   | Character                                                          |
//! |---------|--------------------------------------------------------------------|
//! | Classic | Warm, analog-modeled — FabFilter One heritage. Smooth `tanh`       |
//! |         | resonance feedback with DC-preserving boost. Self-oscillates       |
//! |         | cleanly at full resonance.                                         |
//! | Raw     | Heavy overdrive with aggressive character, great for distortion    |
//! |         | guitar sounds. High pre-gain + an asymmetric hard-knee waveshaper  |
//! |         | creates a thick, harmonically rich texture.                        |
//! | Tube    | Warm asymmetric saturation inspired by triode valve amplifiers.    |
//! |         | A slight positive transfer-curve bias produces 2nd-harmonic        |
//! |         | content — the characteristic warmth of tubes. Great for synth.     |
//! | Clean   | Fully linear — no drive, no clipping, no harmonic distortion.      |
//! |         | Self-oscillates as a pure sine wave at resonance = 1.0.            |
//!
//! All four styles share the same two topologies:
//!   - **Moog Ladder** (LP24, HP24): 4-pole TPT one-pole cascade.
//!   - **Cytomic TPT SVF** (LP12, HP12, BP12, Notch): Andy Simper's ZDF SVF.
//!
//! `FilterEngineV2::process_stereo` / `process_mono` mirror the signatures of
//! `FilterEngine` exactly, with one additional `filter_style: FilterStyle`
//! parameter that selects the nonlinear character.

use nih_plug::prelude::Enum;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

// Re-export the shared enums and structs from `filter.rs` so callers can use a
// single import path.
pub use crate::filter::{
    FilterEnvMode, FilterEnvPhase, FilterEnvelope, FilterPosition, FilterType, SvfOut,
};

// ─── Filter Style ─────────────────────────────────────────────────────────────

/// FabFilter Volcano 3 inspired filter style.
///
/// Selects the nonlinear saturation character applied inside the filter loop.
/// The pole topology and output type (LP / HP / BP / Notch) are still selected
/// by [`FilterType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Enum, Serialize, Deserialize)]
pub enum FilterStyle {
    /// Warm analog character taken from the FabFilter One synthesizer.
    ///
    /// Smooth `tanh` resonance feedback with a DC-preserving (1+k) pre-boost.
    /// Self-oscillates cleanly at full resonance. Slight harmonic saturation at
    /// high drive levels.
    #[default]
    Classic,

    /// Heavy overdrive with aggressive character — great for distortion sounds.
    ///
    /// High pre-gain (≈ ×4 before the saturation function) drives the filter
    /// hard. An asymmetric `x·|x|` term adds both odd and even harmonics,
    /// producing a rough, guitar-amp-like texture.
    Raw,

    /// Warm asymmetric saturation inspired by triode valve amplifiers.
    ///
    /// An `x²` term in the transfer curve (implemented as `x·|x|`, which
    /// preserves sign) creates a small positive bias and strong 2nd-harmonic
    /// content — the defining warmth of class-A tube stages.
    Tube,

    /// Fully linear — no drive, no clipping, no harmonic distortion.
    ///
    /// The drive stage is bypassed entirely. The resonance path contains no
    /// saturation. Self-oscillates as a clean, undistorted sine wave.
    Clean,
}

// ─── Per-style saturation functions ───────────────────────────────────────────

/// Classic: warm `tanh` — the standard Moog / FabFilter analog saturation.
#[inline(always)]
fn classic_sat(x: f32) -> f32 {
    x.tanh()
}

/// Raw: asymmetric high-gain waveshaper.
///
/// `RAW_DRIVE` boosts the signal before `tanh` so it clips hard — practically
/// a square wave at high levels. The `0.15 · x · |x|` term folds both even and
/// odd harmonics into the output for extra grit. The internal division by
/// `RAW_DRIVE` normalises unity gain for small signals.
#[inline(always)]
fn raw_sat(x: f32) -> f32 {
    const RAW_DRIVE: f32 = 3.0;
    // Asymmetric pre-distortion before the hard clip.
    let y = x * RAW_DRIVE + 0.15 * x * x.abs();
    y.tanh() / RAW_DRIVE
}

/// Tube: asymmetric triode transfer curve.
///
/// Simplified valve model:  y = a1·x + a2·x² + a3·x³ → tanh(y)
///   · a2 > 0  (positive bias)  → even harmonics (2nd harmonic warmth)
///   · a3 < 0  (cubic limiting) → soft peak without fold-back
/// Wrapped in `tanh` to guarantee a bounded output.
#[inline(always)]
fn tube_sat(x: f32) -> f32 {
    // x·|x| = sign(x)·x² — adds 2nd-harmonic content while preserving sign.
    let y = x + 0.18 * x * x.abs() - 0.06 * x * x * x;
    y.tanh()
}

// ─── TPT One-Pole ─────────────────────────────────────────────────────────────

/// Single bilinear one-pole filter stage — building block of `MoogLadderV2`.
///
/// Identical to `TptOnePole` in `filter.rs`; kept private and self-contained
/// so `filter_v2.rs` does not depend on the internals of the original module.
#[derive(Clone, Default)]
struct TptOnePoleV2 {
    s: f32,
}

impl TptOnePoleV2 {
    /// Low-pass output.  `g_norm` = g / (1 + g) where g = tan(π × fc / fs).
    #[inline(always)]
    fn lowpass(&mut self, input: f32, g_norm: f32) -> f32 {
        let v = (input - self.s) * g_norm;
        let y = v + self.s;
        self.s = y + v; // TPT state update
        y
    }

    fn clear(&mut self) {
        self.s = 0.0;
    }
}

// ─── Moog Ladder V2 ───────────────────────────────────────────────────────────

/// 4-pole Moog-style ladder filter with style-selectable resonance saturation.
///
/// The topology is identical to `MoogLadder` in `filter.rs` — 4 cascaded TPT
/// one-poles with a DC-preserving `(1+k)` feedback pre-boost — but the
/// saturation function applied to the feedback-subtracted input is chosen per
/// [`FilterStyle`] rather than being hard-coded to `tanh`.
#[derive(Clone, Default)]
pub struct MoogLadderV2 {
    stages: [TptOnePoleV2; 4],
    /// Last LP output, used as the resonance feedback source (Huovilainen style).
    last_lp_output: f32,
}

impl MoogLadderV2 {
    /// Run all 4 ladder stages and return `(raw_lp, comp_factor)`.
    #[inline]
    fn run_stages(
        &mut self,
        input: f32,
        g_norm: f32,
        resonance: f32,
        style: FilterStyle,
    ) -> (f32, f32) {
        let k = resonance * 3.95;

        // DC-preserving feedback: keeps bass gain at unity while driving the
        // saturation progressively harder as resonance increases.
        let pre = input * (1.0 + k) - k * self.last_lp_output;

        let x = match style {
            FilterStyle::Classic => classic_sat(pre),

            FilterStyle::Raw => {
                // Extra 1.4× boost on top of the standard (1+k) factor for a
                // harder, more overdriven entry into the nonlinearity.
                raw_sat(pre * 1.4)
            }

            FilterStyle::Tube => tube_sat(pre),

            FilterStyle::Clean => {
                // No saturation. The hard clamp is a safety rail only — it
                // prevents unbounded growth if the loop is marginally unstable
                // but does not introduce audible harmonic content at normal
                // operating levels.
                pre.clamp(-1.5, 1.5)
            }
        };

        let s0 = self.stages[0].lowpass(x, g_norm);
        let s1 = self.stages[1].lowpass(s0, g_norm);
        let s2 = self.stages[2].lowpass(s1, g_norm);
        let s3 = self.stages[3].lowpass(s2, g_norm);

        self.last_lp_output = s3;

        // Resonance compensation: keeps the output level stable as Q rises.
        // Raw gets slightly more compensation to offset its additional pre-gain.
        let comp = match style {
            FilterStyle::Raw => 1.0 / (1.0 + resonance * resonance * 1.3),
            _ => 1.0 / (1.0 + resonance * resonance),
        };

        (s3, comp)
    }

    /// Low-pass 4-pole output.
    #[inline]
    pub fn process_lp(
        &mut self,
        input: f32,
        g_norm: f32,
        resonance: f32,
        style: FilterStyle,
    ) -> f32 {
        let (lp, comp) = self.run_stages(input, g_norm, resonance, style);
        lp * comp
    }

    /// High-pass 4-pole output.
    ///
    /// Derived as `(input − raw_lp) × comp` to cancel DC correctly at all
    /// resonance settings (same reasoning as `MoogLadder::process_hp`).
    #[inline]
    pub fn process_hp(
        &mut self,
        input: f32,
        g_norm: f32,
        resonance: f32,
        style: FilterStyle,
    ) -> f32 {
        let (lp, comp) = self.run_stages(input, g_norm, resonance, style);
        (input - lp) * comp
    }

    pub fn clear(&mut self) {
        for s in &mut self.stages {
            s.clear();
        }
        self.last_lp_output = 0.0;
    }
}

// ─── Cytomic TPT SVF V2 ───────────────────────────────────────────────────────

/// Cytomic TPT state-variable filter with style-selectable BP-integrator
/// saturation.
///
/// The LP integrator (IC2) remains unconditionally linear to preserve pole
/// stability. Only the BP integrator (IC1) — which carries the resonance peak —
/// receives the style-specific nonlinearity. This shapes the resonant character
/// without destabilising the filter.
///
/// Reference: Andy Simper, "Solving the Continuous SVF Equations Using
/// Trapezoidal Integration and its Application to Audio Processing", 2014.
#[derive(Clone, Default)]
pub struct TptSvfV2 {
    ic1eq: f32,
    ic2eq: f32,
}

impl TptSvfV2 {
    /// Process one sample with style-specific resonance saturation.
    ///
    /// - `g`               = tan(π × fc / fs)
    /// - `k`               = 1/Q (= 2 − 1.98 × resonance, clamped to 0.02)
    /// - `resonance_drive` = resonance² × drive_scale  (0.0 = clean)
    /// - `style`           = nonlinear character selector
    #[inline]
    pub fn process_styled(
        &mut self,
        input: f32,
        g: f32,
        k: f32,
        resonance_drive: f32,
        style: FilterStyle,
    ) -> SvfOut {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;

        // Apply per-style saturation to the BP integrator update.
        //
        // All branches preserve unity gain for small signals: when the signal
        // is below the saturation threshold, `sat(bp * ds) / ds ≈ bp`.
        let bp_update = 2.0 * v1 - self.ic1eq;
        self.ic1eq = match style {
            FilterStyle::Classic => {
                if resonance_drive > 1e-4 {
                    // Same drive/compression curve as `filter.rs` for a
                    // consistent, familiar Classic character.
                    let ds = 1.0 + resonance_drive * 4.0;
                    (bp_update * ds).tanh() / ds
                } else {
                    bp_update
                }
            }

            FilterStyle::Raw => {
                if resonance_drive > 1e-4 {
                    // Higher drive scale → harder, more aggressive clipping at
                    // the resonance peak. The asymmetric term in `raw_sat`
                    // adds extra harmonic grit.
                    let ds = 1.0 + resonance_drive * 8.0;
                    // raw_sat has unity small-signal gain, so dividing by ds
                    // restores unity gain for signals below clipping threshold.
                    raw_sat(bp_update * ds) / ds
                } else {
                    bp_update
                }
            }

            FilterStyle::Tube => {
                if resonance_drive > 1e-4 {
                    // Moderate drive with tube asymmetry — slightly warmer
                    // than Classic at the resonance peak.
                    let ds = 1.0 + resonance_drive * 4.5;
                    tube_sat(bp_update * ds) / ds
                } else {
                    bp_update
                }
            }

            // Clean: no saturation at all. The BP integrator is purely linear.
            FilterStyle::Clean => bp_update,
        };

        // LP integrator is unconditionally linear — keeps pole structure stable.
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        SvfOut {
            lp: v2,
            bp: v1,
            hp: input - k * v1 - v2,
            notch: input - k * v1,
        }
    }

    pub fn clear(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }
}

// ─── Filter Engine V2 ─────────────────────────────────────────────────────────

/// Unified stereo filter engine — v2 with FabFilter Volcano 3 style selection.
///
/// Drop-in upgrade for `FilterEngine` (from `filter.rs`), with one additional
/// `filter_style: FilterStyle` parameter on `process_stereo` / `process_mono`.
///
/// - `process_mono()`  : for PreNam and PostNam chain positions (mono signal).
/// - `process_stereo()`: for PostAll chain position (stereo bus).
#[derive(Clone)]
pub struct FilterEngineV2 {
    ladder_l: MoogLadderV2,
    ladder_r: MoogLadderV2,
    svf_l: TptSvfV2,
    svf_r: TptSvfV2,
    pub envelope: FilterEnvelope,
}

impl Default for FilterEngineV2 {
    fn default() -> Self {
        Self {
            ladder_l: MoogLadderV2::default(),
            ladder_r: MoogLadderV2::default(),
            svf_l: TptSvfV2::default(),
            svf_r: TptSvfV2::default(),
            envelope: FilterEnvelope::default(),
        }
    }
}

impl FilterEngineV2 {
    pub fn trigger(&mut self, velocity: f32) {
        self.envelope.trigger(velocity);
    }

    pub fn release(&mut self) {
        self.envelope.release();
    }

    /// Clear all integrator state and the envelope.
    ///
    /// Call when the filter type, position, or style changes to avoid stale-
    /// state transients (pops / clicks).
    pub fn clear(&mut self) {
        self.ladder_l.clear();
        self.ladder_r.clear();
        self.svf_l.clear();
        self.svf_r.clear();
        self.envelope.clear();
    }

    /// Process one stereo sample pair.  Advances the envelope by one sample.
    ///
    /// Use at the **PostAll** position (after Corrosion, on the stereo bus).
    ///
    /// # Parameters
    ///
    /// Identical to `FilterEngine::process_stereo` except for the additional
    /// `filter_style` argument.
    #[inline]
    pub fn process_stereo(
        &mut self,
        l: f32,
        r: f32,
        sample_rate: f32,
        filter_type: FilterType,
        filter_style: FilterStyle,
        cutoff_hz: f32,
        resonance: f32,
        env_amount_oct: f32,
        env_attack_ms: f32,
        env_decay_ms: f32,
        env_sustain: f32,
        env_release_ms: f32,
        drive_db: f32,
        midi_note: u8,
        key_track: f32,
    ) -> (f32, f32) {
        let env_val = self.envelope.tick(
            sample_rate,
            env_attack_ms,
            env_decay_ms,
            env_sustain,
            env_release_ms,
        );

        let effective_hz = compute_cutoff(
            cutoff_hz,
            env_val,
            env_amount_oct,
            midi_note,
            key_track,
            sample_rate,
        );

        let g = (PI * effective_hz / sample_rate).tan().max(1e-6);
        let g_norm = g / (1.0 + g);
        let k = (2.0 - 1.98 * resonance.clamp(0.0, 1.0)).max(0.02);

        // Clean bypasses the drive stage entirely — preserves linear behaviour.
        let (dl, dr) = if filter_style == FilterStyle::Clean {
            (l, r)
        } else {
            (apply_drive(l, drive_db), apply_drive(r, drive_db))
        };

        // Resonance-driven saturation amount. Zero for Clean (fully linear).
        let res_drive = if filter_style == FilterStyle::Clean {
            0.0
        } else {
            resonance.powi(2) * (1.0 + drive_db.min(24.0) / 48.0)
        };

        match filter_type {
            FilterType::LP24 => (
                self.ladder_l
                    .process_lp(dl, g_norm, resonance, filter_style),
                self.ladder_r
                    .process_lp(dr, g_norm, resonance, filter_style),
            ),
            FilterType::HP24 => (
                self.ladder_l
                    .process_hp(dl, g_norm, resonance, filter_style),
                self.ladder_r
                    .process_hp(dr, g_norm, resonance, filter_style),
            ),
            FilterType::LP12 => {
                let ol = self.svf_l.process_styled(dl, g, k, res_drive, filter_style);
                let or_ = self.svf_r.process_styled(dr, g, k, res_drive, filter_style);
                (ol.lp, or_.lp)
            }
            FilterType::HP12 => {
                let ol = self.svf_l.process_styled(dl, g, k, res_drive, filter_style);
                let or_ = self.svf_r.process_styled(dr, g, k, res_drive, filter_style);
                (ol.hp, or_.hp)
            }
            FilterType::BP12 => {
                let ol = self.svf_l.process_styled(dl, g, k, res_drive, filter_style);
                let or_ = self.svf_r.process_styled(dr, g, k, res_drive, filter_style);
                (ol.bp, or_.bp)
            }
            FilterType::Notch => {
                let ol = self.svf_l.process_styled(dl, g, k, res_drive, filter_style);
                let or_ = self.svf_r.process_styled(dr, g, k, res_drive, filter_style);
                (ol.notch, or_.notch)
            }
        }
    }

    /// Process one mono sample.  Advances the envelope by one sample.
    ///
    /// Use at **PreNam** and **PostNam** positions (signal is still mono).
    /// The right-channel filter state is kept in sync so a seamless transition
    /// to `process_stereo` (PostAll) is possible without clicks.
    #[inline]
    pub fn process_mono(
        &mut self,
        input: f32,
        sample_rate: f32,
        filter_type: FilterType,
        filter_style: FilterStyle,
        cutoff_hz: f32,
        resonance: f32,
        env_amount_oct: f32,
        env_attack_ms: f32,
        env_decay_ms: f32,
        env_sustain: f32,
        env_release_ms: f32,
        drive_db: f32,
        midi_note: u8,
        key_track: f32,
    ) -> f32 {
        let env_val = self.envelope.tick(
            sample_rate,
            env_attack_ms,
            env_decay_ms,
            env_sustain,
            env_release_ms,
        );

        let effective_hz = compute_cutoff(
            cutoff_hz,
            env_val,
            env_amount_oct,
            midi_note,
            key_track,
            sample_rate,
        );

        let g = (PI * effective_hz / sample_rate).tan().max(1e-6);
        let g_norm = g / (1.0 + g);
        let k = (2.0 - 1.98 * resonance.clamp(0.0, 1.0)).max(0.02);

        let x = if filter_style == FilterStyle::Clean {
            input
        } else {
            apply_drive(input, drive_db)
        };

        let res_drive = if filter_style == FilterStyle::Clean {
            0.0
        } else {
            resonance.powi(2) * (1.0 + drive_db.min(24.0) / 48.0)
        };

        let out = match filter_type {
            FilterType::LP24 => self.ladder_l.process_lp(x, g_norm, resonance, filter_style),
            FilterType::HP24 => self.ladder_l.process_hp(x, g_norm, resonance, filter_style),
            FilterType::LP12 => {
                self.svf_l
                    .process_styled(x, g, k, res_drive, filter_style)
                    .lp
            }
            FilterType::HP12 => {
                self.svf_l
                    .process_styled(x, g, k, res_drive, filter_style)
                    .hp
            }
            FilterType::BP12 => {
                self.svf_l
                    .process_styled(x, g, k, res_drive, filter_style)
                    .bp
            }
            FilterType::Notch => {
                self.svf_l
                    .process_styled(x, g, k, res_drive, filter_style)
                    .notch
            }
        };

        // Mirror left state into right so the stereo channels stay in sync
        // when the user switches the filter position to PostAll.
        self.ladder_r.stages.clone_from(&self.ladder_l.stages);
        self.ladder_r.last_lp_output = self.ladder_l.last_lp_output;
        self.svf_r.ic1eq = self.svf_l.ic1eq;
        self.svf_r.ic2eq = self.svf_l.ic2eq;

        out
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Fraction of Nyquist used as the cutoff ceiling in `compute_cutoff`.
///
/// Keeps the Moog ladder well below Nyquist (where g_norm → 1 and the
/// resonance peak becomes very loud regardless of the resonance setting).
/// Matches the value used in `filter.rs`.
const NYQUIST_SAFETY_FACTOR: f32 = 0.425;

/// Compute the effective cutoff frequency (Hz) with envelope and key-tracking
/// applied.  Matches `compute_cutoff` in `filter.rs` exactly.
#[inline]
fn compute_cutoff(
    base_hz: f32,
    env_val: f32,
    env_amount_oct: f32,
    midi_note: u8,
    key_track: f32,
    sample_rate: f32,
) -> f32 {
    let note_hz = 440.0 * 2.0f32.powf((midi_note as f32 - 69.0) / 12.0);
    let tracked_hz = base_hz * (1.0 - key_track) + note_hz * key_track;
    let ceiling = (sample_rate * NYQUIST_SAFETY_FACTOR).min(18_000.0_f32);
    (tracked_hz * 2.0f32.powf(env_val * env_amount_oct)).clamp(20.0, ceiling)
}

/// Pre-filter drive stage.  Identical to `apply_drive` in `filter.rs`.
///
/// At `drive_db` = 0.0 this is a no-op (returns `input` unchanged).
/// Above 0 dB the signal is amplified and then soft-clipped with `tanh`.
/// Not called for [`FilterStyle::Clean`] — that path bypasses drive entirely.
#[inline]
fn apply_drive(input: f32, drive_db: f32) -> f32 {
    if drive_db < 0.1 {
        return input;
    }
    let gain = 10f32.powf(drive_db / 20.0);
    (input * gain).tanh()
}
