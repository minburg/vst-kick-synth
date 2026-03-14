//! High-quality filter engine for the Kick Synth.
//!
//! Two filter architectures:
//!   - **Moog Ladder** (LP24, HP24): 4-pole TPT one-pole cascade with resonance
//!     feedback and tanh saturation. Self-oscillates at resonance = 1.0.
//!   - **Cytomic TPT SVF** (LP12, HP12, BP12, Notch): Andy Simper's topology-
//!     preserving transform state-variable filter. Simultaneously produces all
//!     four outputs from a single two-integrator loop.
//!
//! Both use bilinear (TPT / ZDF) integrators to minimise aliasing and guarantee
//! stability independent of cutoff frequency.

use nih_plug::prelude::Enum;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

// ─── Public Enums ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Enum, Serialize, Deserialize)]
pub enum FilterType {
    /// 4-pole Moog Ladder lowpass. Warm, self-oscillates at resonance = 1.0.
    #[default]
    #[name = "LP 24"]
    LP24,
    /// 2-pole TPT SVF lowpass.
    #[name = "LP 12"]
    LP12,
    /// 4-pole highpass (Moog Ladder topology).
    #[name = "HP 24"]
    HP24,
    /// 2-pole TPT SVF highpass.
    #[name = "HP 12"]
    HP12,
    /// 2-pole TPT SVF bandpass.
    #[name = "BP 12"]
    BP12,
    /// 2-pole TPT SVF notch (band-reject).
    #[name = "Notch"]
    Notch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Enum, Serialize, Deserialize)]
pub enum FilterPosition {
    /// Applied after oscillator synthesis, before the NAM model.
    /// Shapes the tone before saturation — tighter, darker results.
    #[name = "Pre NAM"]
    PreNam,
    /// Applied after the NAM model, before the Corrosion stage (default).
    /// Most natural position for post-distortion tone shaping.
    #[default]
    #[name = "Post NAM"]
    PostNam,
    /// Applied after ALL processing including Corrosion, on the stereo bus.
    /// Acts as a master filter / tone control.
    #[name = "Post All"]
    PostAll,
}

// ─── TPT One-Pole ─────────────────────────────────────────────────────────────

/// Single bilinear one-pole filter stage — building block of the Moog Ladder.
#[derive(Clone, Default)]
pub struct TptOnePole {
    /// Integrator state (the "capacitor voltage").
    s: f32,
}

impl TptOnePole {
    /// Low-pass output. `g_norm` = g / (1 + g) where g = tan(π × fc / fs).
    #[inline(always)]
    pub fn lowpass(&mut self, input: f32, g_norm: f32) -> f32 {
        let v = (input - self.s) * g_norm;
        let y = v + self.s;
        self.s = y + v; // state = 2y − prev_state (TPT update)
        y
    }

    /// High-pass output (input − low-pass).
    #[inline(always)]
    pub fn highpass(&mut self, input: f32, g_norm: f32) -> f32 {
        input - self.lowpass(input, g_norm)
    }

    pub fn clear(&mut self) {
        self.s = 0.0;
    }
}

// ─── Moog Ladder (LP24 / HP24) ────────────────────────────────────────────────

/// 4-pole Moog-style ladder filter.
///
/// Uses 4 cascaded TPT one-poles with a resonance feedback path and tanh
/// saturation on the feedback-subtracted input. Self-oscillates cleanly at
/// `resonance` = 1.0.
#[derive(Clone, Default)]
pub struct MoogLadder {
    stages: [TptOnePole; 4],
    /// Last LP output sample, used as the resonance feedback source.
    ///
    /// # Why output and not `stages[3].s` (the integrator state)?
    ///
    /// The TPT one-pole integrator state `s` and the stage output `y` have
    /// different frequency responses:
    ///
    ///   |H_state(ω)|  = 2·g_norm / |1 − (1−2·g_norm)·e^{−jω}|
    ///   |H_output(ω)| = g_norm · |1 + e^{−jω}| / |1 − (1−2·g_norm)·e^{−jω}|
    ///
    /// At low-to-mid frequencies both are approximately equal; at frequencies
    /// approaching Nyquist the state can be **several times larger** than the
    /// output (e.g. ×2.4 per stage at 17 kHz / 44.1 kHz, ×3+ at higher freqs).
    /// Feeding the bloated state value back with `resonance × 3.95` causes the
    /// loop gain to approach 1.0 even at modest resonance settings whenever the
    /// filter cutoff is pushed toward Nyquist by a large envelope amount — the
    /// symptom being a loud high-pitch whistle at low resonance values.
    ///
    /// Using the actual last LP output (one-sample delayed, standard Huovilainen
    /// approach) makes the feedback magnitude naturally taper off near Nyquist,
    /// matching what a real analogue ladder does.
    last_lp_output: f32,
}

impl MoogLadder {
    /// Run the 4 ladder stages and return `(raw_lp, comp_factor)`.
    ///
    /// Private helper shared by `process_lp` and `process_hp`.  Both outputs
    /// are derived from the **same** single pass through the stages so neither
    /// LP nor HP causes a double-advance of the filter state.
    #[inline]
    fn run_stages(&mut self, input: f32, g_norm: f32, resonance: f32) -> (f32, f32) {
        let k = resonance * 3.95;

        // ── DC-preserving feedback (FabFilter Simplon / Saturn 2 style) ──────
        //
        // Standard Moog:  x = tanh(input         − k × last_out)
        //   → At DC, gain = 1/(1+k).  At resonance = 0.6 this is −10 dB of
        //     bass loss; the kick body thins out dramatically.
        //
        // This implementation:
        //   x = tanh(input × (1+k) − k × last_out)
        //   → At DC (linear approx): output = input*(1+k)/(1+k) = input → gain = 1.0
        //     Bass stays at its original level regardless of resonance. ✓
        //
        // The (1+k) pre-boost has a second, intentional effect:
        //   It drives `tanh` progressively harder as resonance increases,
        //   generating harmonics in the resonant band rather than producing a
        //   dominant, narrow peak.  At typical kick amplitudes (0.2–0.4) the
        //   saturation is subtle at low resonance and increasingly rich at
        //   high resonance — the "warm harmonic saturation" of Simplon Raw.
        //
        // At very high resonance the system self-oscillates, but the tanh on
        // the loop input keeps the amplitude bounded, matching real analogue
        // ladder behaviour.
        let x = (input * (1.0 + k) - k * self.last_lp_output).tanh();

        let s0 = self.stages[0].lowpass(x, g_norm);
        let s1 = self.stages[1].lowpass(s0, g_norm);
        let s2 = self.stages[2].lowpass(s1, g_norm);
        let s3 = self.stages[3].lowpass(s2, g_norm);

        // Store RAW output for the feedback path (no comp applied to feedback).
        self.last_lp_output = s3;

        // Moderate output level: the pre-boost raises the signal going through
        // the ladder, so a gentle gain correction keeps the output consistent
        // with what the dry-level knobs imply.  The curve is soft so the
        // resonance is still clearly audible — it just no longer dominates.
        //   r = 0.0 → comp = 1.00   r = 0.5 → comp = 0.80
        //   r = 0.7 → comp = 0.67   r = 1.0 → comp = 0.50
        let comp = 1.0 / (1.0 + resonance * resonance);

        (s3, comp)
    }

    /// Low-pass 4-pole output.
    ///
    /// - `g_norm` = g / (1 + g) where g = tan(π × fc / fs)
    /// - `resonance`: 0.0 (clean) → 1.0 (self-oscillation)
    #[inline]
    pub fn process_lp(&mut self, input: f32, g_norm: f32, resonance: f32) -> f32 {
        let (lp, comp) = self.run_stages(input, g_norm, resonance);
        lp * comp
    }

    /// High-pass 4-pole output.
    ///
    /// HP is derived as `(input − raw_lp) × comp` — **not** `input − lp*comp`.
    ///
    /// Why this matters: with the DC-preserving Moog (raw LP DC gain ≈ 1.0),
    /// `input − lp*comp` leaves a residual DC component of `input × (1−comp)`
    /// that increases with resonance — i.e. the HP bleeds low end at high Q.
    /// Using `(input − raw_lp) × comp` cancels DC correctly: at DC,
    /// `(input − input) × comp = 0`, regardless of the comp value. ✓
    #[inline]
    pub fn process_hp(&mut self, input: f32, g_norm: f32, resonance: f32) -> f32 {
        let (lp, comp) = self.run_stages(input, g_norm, resonance);
        (input - lp) * comp
    }

    pub fn clear(&mut self) {
        for s in &mut self.stages {
            s.clear();
        }
        self.last_lp_output = 0.0;
    }
}

// ─── Cytomic TPT SVF (LP12 / HP12 / BP12 / Notch) ───────────────────────────

/// All four outputs (LP, HP, BP, Notch) from a single pass.
#[derive(Clone, Copy, Default)]
pub struct SvfOut {
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
    pub notch: f32,
}

/// Cytomic topology-preserving transform state-variable filter.
///
/// Reference: Andy Simper, "Solving the Continuous SVF Equations Using
/// Trapezoidal Integration and its Application to Audio Processing", 2014.
#[derive(Clone, Default)]
pub struct TptSvf {
    ic1eq: f32,
    ic2eq: f32,
}

impl TptSvf {
    /// Process one sample and return all four topology outputs simultaneously.
    ///
    /// - `g`  = tan(π × fc / fs)
    /// - `k`  = 1/Q = 2 − 2 × resonance  (k → 0 at high resonance, k = 2 at r = 0)
    #[inline]
    pub fn process(&mut self, input: f32, g: f32, k: f32) -> SvfOut {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;

        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        SvfOut {
            lp: v2,
            bp: v1,
            hp: input - k * v1 - v2,
            notch: input - k * v1, // lp + hp
        }
    }

    /// Process with nonlinear BP-integrator saturation — Simplon / 303 style.
    ///
    /// At high resonance (`k` near 0) the bandpass path naturally builds large
    /// amplitude.  Applying `tanh` to the IC1 update limits that peak and folds
    /// energy into harmonics, producing the characteristic "filtered distortion"
    /// heard in analogue resonant filters at high Q.
    ///
    /// - `resonance_drive`: 0.0 = fully clean (identical to `process`);
    ///   1.0 = saturates at moderate signal levels; scale with resonance².
    ///   Only IC1 (the BP integrator) is clipped; IC2 (LP) stays linear for
    ///   stability.
    #[inline]
    pub fn process_nl(&mut self, input: f32, g: f32, k: f32, resonance_drive: f32) -> SvfOut {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;

        // Saturate only the bandpass (resonance) integrator.
        // The scale factor preserves unity gain for small signals so the
        // character only emerges when the resonance peak is actually large.
        let bp_update = 2.0 * v1 - self.ic1eq;
        if resonance_drive > 1e-4 {
            let drive_scale = 1.0 + resonance_drive * 4.0;
            self.ic1eq = (bp_update * drive_scale).tanh() / drive_scale;
        } else {
            self.ic1eq = bp_update;
        }
        // LP integrator is unconditionally linear — keeps the pole structure stable.
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

// ─── Filter Envelope ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum FilterEnvPhase {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR envelope for filter cutoff modulation.
///
/// Velocity-sensitive: `trigger(velocity)` scales the envelope peak by the
/// note velocity, so harder hits produce more modulation.
/// Click-free re-trigger: starting from the current value instead of 0.
/// How the filter envelope responds to note-off.
///
/// - **Trigger** (default for kick drums): the envelope fires and completes
///   its full A→D→R cycle independently of note length.  After the decay
///   settles at the sustain level the release phase begins automatically,
///   so the filter always closes on its own — exactly like Kick 2/3, the
///   Roland 808, and the "trigger" mode in Serum / Vital.
///
/// - **Gate**: classic synthesiser behaviour — the sustain phase is held for
///   as long as the MIDI note is held, release starts on note-off.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterEnvMode {
    #[default]
    Trigger,
    Gate,
}

#[derive(Clone)]
pub struct FilterEnvelope {
    pub phase: FilterEnvPhase,
    pub value: f32,
    pub mode: FilterEnvMode,
    phase_timer: f32,
    release_coeff: f32,
    velocity_scale: f32,
}

impl Default for FilterEnvelope {
    fn default() -> Self {
        Self {
            phase: FilterEnvPhase::Idle,
            value: 0.0,
            mode: FilterEnvMode::Trigger, // fire-and-forget: best for kick drums
            phase_timer: 0.0,
            release_coeff: 0.0,
            velocity_scale: 1.0,
        }
    }
}

impl FilterEnvelope {
    pub fn trigger(&mut self, velocity: f32) {
        self.phase = FilterEnvPhase::Attack;
        self.phase_timer = 0.0;
        self.velocity_scale = velocity;
        // value stays at current for click-free re-trigger
    }

    pub fn release(&mut self) {
        if self.phase != FilterEnvPhase::Idle && self.phase != FilterEnvPhase::Release {
            self.phase = FilterEnvPhase::Release;
            self.phase_timer = 0.0;
        }
    }

    /// Advance the envelope by one sample; returns the current value (0..1).
    ///
    /// Both attack and decay use **exponential curves**, matching the response
    /// of classic analog filter envelopes (Moog, Roland 303/808/909 style).
    ///
    /// - Attack: logarithmic (fast initial snap → asymptotic approach to peak).
    ///   Gives an instantaneous "crack-open" feeling on short attacks.
    /// - Decay: exponential (fast initial drop → gradually settles at sustain).
    ///   This is the critical shape for the punchy "wah then close" kick sound —
    ///   the filter sweeps hard in the first few milliseconds and then tails off.
    #[inline]
    pub fn tick(
        &mut self,
        sample_rate: f32,
        attack_ms: f32,
        decay_ms: f32,
        sustain: f32,
        release_ms: f32,
    ) -> f32 {
        match self.phase {
            FilterEnvPhase::Attack => {
                let samples = (sample_rate * attack_ms / 1000.0).max(1.0);
                // Exponential approach to 1.0: coeff chosen so we reach 99.9 %
                // after attack_ms (ln(0.001) ≈ −6.9).
                let coeff = (-6.9_f32 / samples).exp();
                self.value = 1.0 - (1.0 - self.value) * coeff;
                if self.value >= 1.0 - 1e-4 {
                    self.value = 1.0;
                    self.phase = FilterEnvPhase::Decay;
                    self.phase_timer = 0.0;
                }
            }
            FilterEnvPhase::Decay => {
                let samples = (sample_rate * decay_ms / 1000.0).max(1.0);
                // Exponential decay toward sustain: fast initial drop, slow tail.
                // This is the defining characteristic of classic drum-machine filter
                // envelopes — the frequency sweeps hard at the start and gradually
                // settles, giving the characteristic "snap" of the Moog / 303 / 808.
                let coeff = (-6.9_f32 / samples).exp();
                self.value = sustain + (self.value - sustain) * coeff;
                if (self.value - sustain).abs() < 1e-4 {
                    self.value = sustain;
                    self.phase = if sustain > 1e-4 {
                        FilterEnvPhase::Sustain
                    } else {
                        FilterEnvPhase::Idle
                    };
                }
            }
            FilterEnvPhase::Sustain => {
                match self.mode {
                    FilterEnvMode::Trigger => {
                        // Fire-and-forget: skip holding at sustain — immediately
                        // begin the release tail.  The release sweeps from the
                        // sustain level to 0, giving a two-stage close-down:
                        //   fast main decay  →  slower tail-off
                        // This matches how Kick 2/3, the 808, and Serum's
                        // "trigger" mode work: the filter always closes fully
                        // on its own regardless of how long the note is held.
                        self.phase = FilterEnvPhase::Release;
                        self.phase_timer = 0.0;
                    }
                    FilterEnvMode::Gate => {
                        // Hold at sustain level until note-off.
                        self.value = sustain;
                    }
                }
            }
            FilterEnvPhase::Release => {
                if self.phase_timer == 0.0 {
                    let samples = (sample_rate * release_ms / 1000.0).max(1.0);
                    self.release_coeff = 0.0001f32.powf(1.0 / samples);
                }
                self.value *= self.release_coeff;
                self.phase_timer += 1.0;
                if self.value < 1e-6 {
                    self.value = 0.0;
                    self.phase = FilterEnvPhase::Idle;
                }
            }
            FilterEnvPhase::Idle => {
                self.value = 0.0;
            }
        }
        self.value * self.velocity_scale
    }

    pub fn clear(&mut self) {
        self.phase = FilterEnvPhase::Idle;
        self.value = 0.0;
        self.phase_timer = 0.0;
    }
}

// ─── Filter Engine ────────────────────────────────────────────────────────────

/// Unified stereo filter engine.
///
/// Contains a stereo pair (L + R) of both filter architectures plus the shared
/// ADSR envelope. The active architecture is chosen by `FilterType` at runtime.
///
/// - `process_mono()`: for PreNam and PostNam chain positions (mono signal).
/// - `process_stereo()`: for PostAll chain position (stereo bus).
#[derive(Clone)]
pub struct FilterEngine {
    ladder_l: MoogLadder,
    ladder_r: MoogLadder,
    svf_l: TptSvf,
    svf_r: TptSvf,
    pub envelope: FilterEnvelope,
}

impl Default for FilterEngine {
    fn default() -> Self {
        Self {
            ladder_l: MoogLadder::default(),
            ladder_r: MoogLadder::default(),
            svf_l: TptSvf::default(),
            svf_r: TptSvf::default(),
            envelope: FilterEnvelope::default(),
        }
    }
}

impl FilterEngine {
    pub fn trigger(&mut self, velocity: f32) {
        self.envelope.trigger(velocity);
    }

    pub fn release(&mut self) {
        self.envelope.release();
    }

    /// Clear all filter state (integrators + envelope).
    /// Call when the filter type or position changes to avoid stale state.
    pub fn clear(&mut self) {
        self.ladder_l.clear();
        self.ladder_r.clear();
        self.svf_l.clear();
        self.svf_r.clear();
        self.envelope.clear();
    }

    /// Process one stereo sample. Advances the envelope by one sample.
    ///
    /// Use at **PostAll** position (after Corrosion, on the stereo bus).
    #[inline]
    pub fn process_stereo(
        &mut self,
        l: f32,
        r: f32,
        sample_rate: f32,
        filter_type: FilterType,
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
        let env_val = self
            .envelope
            .tick(sample_rate, env_attack_ms, env_decay_ms, env_sustain, env_release_ms);

        let effective_hz =
            compute_cutoff(cutoff_hz, env_val, env_amount_oct, midi_note, key_track, sample_rate);

        let g = (PI * effective_hz / sample_rate).tan().max(1e-6);
        let g_norm = g / (1.0 + g); // Moog one-pole coefficient
        let k = (2.0 - 1.98 * resonance.clamp(0.0, 1.0)).max(0.02); // SVF Q factor

        // Resonance-driven saturation for SVF types: squared curve keeps it
        // subtle until high Q, then grows significantly — mimics Simplon / 303.
        // The filter_drive parameter also contributes: a hot drive + high Q
        // gives strong, musical harmonic saturation.
        let res_drive = resonance.powi(2) * (1.0 + drive_db.min(24.0) / 48.0);

        let (dl, dr) = (apply_drive(l, drive_db), apply_drive(r, drive_db));

        match filter_type {
            FilterType::LP24 => (
                self.ladder_l.process_lp(dl, g_norm, resonance),
                self.ladder_r.process_lp(dr, g_norm, resonance),
            ),
            FilterType::HP24 => (
                self.ladder_l.process_hp(dl, g_norm, resonance),
                self.ladder_r.process_hp(dr, g_norm, resonance),
            ),
            FilterType::LP12 => {
                let ol = self.svf_l.process_nl(dl, g, k, res_drive);
                let or_ = self.svf_r.process_nl(dr, g, k, res_drive);
                (ol.lp, or_.lp)
            }
            FilterType::HP12 => {
                let ol = self.svf_l.process_nl(dl, g, k, res_drive);
                let or_ = self.svf_r.process_nl(dr, g, k, res_drive);
                (ol.hp, or_.hp)
            }
            FilterType::BP12 => {
                let ol = self.svf_l.process_nl(dl, g, k, res_drive);
                let or_ = self.svf_r.process_nl(dr, g, k, res_drive);
                (ol.bp, or_.bp)
            }
            FilterType::Notch => {
                let ol = self.svf_l.process_nl(dl, g, k, res_drive);
                let or_ = self.svf_r.process_nl(dr, g, k, res_drive);
                (ol.notch, or_.notch)
            }
        }
    }

    /// Process one mono sample. Advances the envelope by one sample.
    ///
    /// Use at **PreNam** and **PostNam** positions (signal is still mono).
    /// The right-channel filter state is kept in sync for a seamless transition
    /// if the user switches to PostAll (stereo) position.
    #[inline]
    pub fn process_mono(
        &mut self,
        input: f32,
        sample_rate: f32,
        filter_type: FilterType,
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
        let env_val = self
            .envelope
            .tick(sample_rate, env_attack_ms, env_decay_ms, env_sustain, env_release_ms);

        let effective_hz =
            compute_cutoff(cutoff_hz, env_val, env_amount_oct, midi_note, key_track, sample_rate);

        let g = (PI * effective_hz / sample_rate).tan().max(1e-6);
        let g_norm = g / (1.0 + g);
        let k = (2.0 - 1.98 * resonance.clamp(0.0, 1.0)).max(0.02);

        // Same resonance-drive calculation as the stereo path for consistency.
        let res_drive = resonance.powi(2) * (1.0 + drive_db.min(24.0) / 48.0);

        let x = apply_drive(input, drive_db);

        let out = match filter_type {
            FilterType::LP24 => self.ladder_l.process_lp(x, g_norm, resonance),
            FilterType::HP24 => self.ladder_l.process_hp(x, g_norm, resonance),
            FilterType::LP12 => self.svf_l.process_nl(x, g, k, res_drive).lp,
            FilterType::HP12 => self.svf_l.process_nl(x, g, k, res_drive).hp,
            FilterType::BP12 => self.svf_l.process_nl(x, g, k, res_drive).bp,
            FilterType::Notch => self.svf_l.process_nl(x, g, k, res_drive).notch,
        };

        // Mirror left state into right so the stereo channels are in sync when
        // the user switches the position to PostAll.
        self.ladder_r.stages.clone_from(&self.ladder_l.stages);
        self.ladder_r.last_lp_output = self.ladder_l.last_lp_output;
        self.svf_r.ic1eq = self.svf_l.ic1eq;
        self.svf_r.ic2eq = self.svf_l.ic2eq;

        out
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Compute the effective cutoff frequency in Hz, applying envelope modulation
/// and MIDI key tracking on top of the base cutoff.
///
/// The ceiling is `sample_rate × 0.425` (≈18.7 kHz at 44.1 kHz, 20.4 kHz at
/// 48 kHz), hard-bounded at 18 000 Hz.  This keeps the Moog ladder well away
/// from Nyquist where g_norm → 1 and the resonance peak would become extremely
/// loud regardless of the resonance setting.
#[inline]
fn compute_cutoff(
    base_hz: f32,
    env_val: f32,
    env_amount_oct: f32,
    midi_note: u8,
    key_track: f32,
    sample_rate: f32,
) -> f32 {
    // Key tracking: blend base cutoff with the MIDI note frequency.
    let note_hz = 440.0 * 2.0f32.powf((midi_note as f32 - 69.0) / 12.0);
    let tracked_hz = base_hz * (1.0 - key_track) + note_hz * key_track;

    // Ceiling: 85 % of Nyquist, hard cap at 18 kHz so resonance peaks stay
    // in a musically useful and controllable range at all sample rates.
    let ceiling = (sample_rate * 0.425).min(18_000.0_f32);

    // Envelope modulation (in octaves): positive amount opens the filter on
    // attack; negative amount closes it.
    (tracked_hz * 2.0f32.powf(env_val * env_amount_oct)).clamp(20.0, ceiling)
}

/// Pre-filter drive stage. Adds harmonic saturation for analog warmth.
///
/// At `drive_db` = 0.0 the function is a no-op (returns `input` unchanged).
/// Above 0 dB the signal is amplified and then soft-clipped with tanh, which
/// keeps the output bounded and generates the characteristic saturation curve.
#[inline]
fn apply_drive(input: f32, drive_db: f32) -> f32 {
    if drive_db < 0.1 {
        return input; // clean path — zero cost at the default setting
    }
    let gain = 10f32.powf(drive_db / 20.0);
    (input * gain).tanh()
}

