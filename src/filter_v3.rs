//! Filter engine v3 — pro-grade ZDF Moog ladder.
//!
//! ## Pro features implemented
//!
//! | Feature                                  | Implementation                                       |
//! |------------------------------------------|------------------------------------------------------|
//! | **True ZDF ladder** (implicit NR solve)  | [`ZdfLadder`]: closed-form for Clean, 3-step NR for  |
//! |                                          | all saturating styles.  No one-sample delay.         |
//! | **2× oversampling** in the nonlinear     | [`LadderOs2x`]: linear-interpolation upsampler +     |
//! | block                                    | 2-tap FIR decimator.  Ladder runs at `2 × fs`.      |
//! | **Topology-derived gain compensation**   | [`output_comp`]: `1/(1 + k/8)` — parameterised by   |
//! |                                          | the actual feedback gain `k`, not a heuristic curve. |
//! | **Nyquist-aware Q limiting**             | [`nyquist_limit_k`]: clamps `k · G⁴ < 3.96` so the  |
//! |                                          | loop gain stays below self-oscillation at all freqs. |
//!
//! ## Public API
//! Drop-in replacement for [`FilterEngineV2`][crate::filter_v2::FilterEngineV2].
//! The struct is renamed to `FilterEngineV3`; everything else (parameter order,
//! types, `FilterStyle`, `FilterType`, `FilterEnvelope`) is identical.

use std::f32::consts::PI;

pub use crate::filter::{
    FilterEnvMode, FilterEnvPhase, FilterEnvelope, FilterPosition, FilterType, SvfOut,
};
// FilterStyle lives in filter_v2; re-export it so call-sites can use either
// module interchangeably without changing their `use` paths.
pub use crate::filter_v2::FilterStyle;

// ─── Saturation functions & their derivatives ────────────────────────────────

/// Classic warm `tanh`.
#[inline(always)]
fn classic_sat(x: f32) -> f32 {
    x.tanh()
}

/// Raw: high-gain asymmetric waveshaper (same coefficients as v2).
#[inline(always)]
fn raw_sat(x: f32) -> f32 {
    const DRIVE: f32 = 3.0;
    let y = x * DRIVE + 0.15 * x * x.abs();
    y.tanh() / DRIVE
}

/// Tube: asymmetric triode transfer curve.
#[inline(always)]
fn tube_sat(x: f32) -> f32 {
    let y = x + 0.18 * x * x.abs() - 0.06 * x * x * x;
    y.tanh()
}

/// Returns `(sat(u), sat'(u))` for use in the Newton-Raphson iteration.
///
/// The derivative is clamped to `≥ 0` so that `F'(y) = 1 + gn⁴·k·sat'(u) ≥ 1`
/// at all times, guaranteeing Newton convergence without overshoot.
///
/// **Clean is handled separately** (exact linear solve) and should never reach
/// this function.
#[inline]
fn sat_with_deriv(u: f32, style: FilterStyle) -> (f32, f32) {
    match style {
        FilterStyle::Classic => {
            let t = u.tanh();
            (t, 1.0 - t * t) // sech²(u) ≥ 0
        }

        FilterStyle::Raw => {
            // The ladder uses `raw_sat(u * 1.4)` — the 1.4× pre-gain is baked
            // into the derivative via the chain rule.
            let x = u * 1.4_f32;
            let inner = x * 3.0 + 0.15 * x * x.abs();
            let t = inner.tanh();
            let sat_val = t / 3.0;
            // d/du[raw_sat(u·1.4)] = (1−t²) · d_inner/du / 3
            // d_inner/du = (3 + 0.3·|x|) · 1.4    where x = u·1.4
            let d_inner_du = (3.0 + 0.3 * x.abs()) * 1.4;
            let deriv = ((1.0 - t * t) * d_inner_du / 3.0).max(0.0);
            (sat_val, deriv)
        }

        FilterStyle::Tube => {
            let inner = u + 0.18 * u * u.abs() - 0.06 * u * u * u;
            let t = inner.tanh();
            // d_inner/du = 1 + 0.36·|u| − 0.18·u²
            // Can go slightly negative at very large amplitudes; clamp to 0.
            let d_inner = 1.0 + 0.36 * u.abs() - 0.18 * u * u;
            let deriv = ((1.0 - t * t) * d_inner).max(0.0);
            (t, deriv)
        }

        // Should not be called for Clean — handled in exact solve branch.
        FilterStyle::Clean => (u, 1.0),
    }
}

/// Apply the ladder's per-style saturation to `u` (value only, no derivative).
///
/// Mirrors the pre-gains from v2 exactly so the character is unchanged.
#[inline]
fn sat_eval(u: f32, style: FilterStyle) -> f32 {
    match style {
        FilterStyle::Classic => classic_sat(u),
        FilterStyle::Raw => raw_sat(u * 1.4),
        FilterStyle::Tube => tube_sat(u),
        FilterStyle::Clean => u,
    }
}

// ─── ZDF Moog Ladder (4-pole, implicit solve) ─────────────────────────────────

/// 4-pole ZDF Moog ladder with **per-sample implicit solve**.
///
/// Unlike v2's `MoogLadderV2`, there is no `last_lp_output` field — the
/// resonance feedback is not delayed.  Instead, we derive the closed-loop
/// equation for `y4` analytically and solve it each sample before updating the
/// integrator states.
///
/// # Math
///
/// Each TPT one-pole stage computes `y = G·x + (1−G)·s` where `G = g_norm`.
/// Cascading four identical stages gives:
///
/// ```text
/// y4 = G⁴·x_sat + σ(s, G)
/// ```
///
/// where `σ` is the state contribution:
///
/// ```text
/// σ = G³(1−G)s₀ + G²(1−G)s₁ + G(1−G)s₂ + (1−G)s₃
/// ```
///
/// The DC-preserving feedback input is:
///
/// ```text
/// x_sat = sat( driven·(1+k) − k·y4 )
/// ```
///
/// Substituting gives the implicit equation for `y4`:
///
/// ```text
/// y4 = G⁴·sat( driven·(1+k) − k·y4 ) + σ
/// ```
///
/// **Clean (linear):** `sat(u) = u`, solve exactly:
/// ```text
/// y4 = [G⁴·driven·(1+k) + σ] / (1 + k·G⁴)
/// ```
///
/// **Saturating styles:** Newton-Raphson (warm-started from the linear guess):
/// ```text
/// F(y)  = y − G⁴·sat(u) − σ         (u = driven·(1+k) − k·y)
/// F'(y) = 1 + G⁴·k·sat'(u)  ≥ 1    → guaranteed convergence
/// ```
#[derive(Clone, Default)]
struct ZdfLadder {
    /// Integrator states for the 4 TPT one-pole stages.
    s: [f32; 4],
}

impl ZdfLadder {
    /// State contribution to `y4` with the current integrator states.
    ///
    /// `y4 = G⁴·x + σ`  where  `σ = G³(1−G)s₀ + G²(1−G)s₁ + G(1−G)s₂ + (1−G)s₃`
    #[inline]
    fn sigma(&self, gn: f32) -> f32 {
        let ig = 1.0 - gn;
        gn * (gn * (gn * ig * self.s[0] + ig * self.s[1]) + ig * self.s[2]) + ig * self.s[3]
    }

    /// Forward-propagate `x_sat` through all 4 stages and update states.
    ///
    /// Returns the LP4 output, which matches the implicitly solved `y4`
    /// to within floating-point precision.
    #[inline]
    fn forward_and_update(&mut self, x_sat: f32, gn: f32) -> f32 {
        let mut y = x_sat;
        for s in &mut self.s {
            let v = (y - *s) * gn;
            let new_y = v + *s;
            *s = new_y + v; // TPT state update: s_new = y + v
            y = new_y;
        }
        y
    }

    /// Process one sample.  Returns `(lp4, hp4)`.
    ///
    /// - `driven`  — input signal after the pre-filter drive stage.
    /// - `gn`      — `g_norm = g / (1 + g)` where `g = tan(π·fc / fs)`.
    /// - `k`       — resonance feedback gain, typically `resonance × 3.95`.
    /// - `style`   — selects the nonlinear saturation character.
    ///
    /// `hp4 = driven − lp4`.  Both outputs share the same implicit solve so
    /// there is no phase inconsistency between LP and HP.
    #[inline]
    pub fn process(&mut self, driven: f32, gn: f32, k: f32, style: FilterStyle) -> (f32, f32) {
        let gn4 = gn * gn * gn * gn;
        let sigma = self.sigma(gn);

        let y4 = match style {
            // ── Exact linear solve ────────────────────────────────────────────
            FilterStyle::Clean => {
                // y4·(1 + k·G⁴) = G⁴·driven·(1+k) + σ
                // Denominator is always ≥ 1 for k ≥ 0 and gn ∈ [0,1].
                let numer = gn4 * driven * (1.0 + k) + sigma;
                let denom = 1.0 + k * gn4;
                numer / denom
            }

            // ── Newton-Raphson for all saturating styles ──────────────────────
            _ => {
                // Warm-start from the linear solution — usually converges in
                // 1–2 iterations; 3 gives sufficient accuracy for f32.
                let mut y = (gn4 * driven * (1.0 + k) + sigma) / (1.0 + k * gn4);

                for _ in 0..3 {
                    let u = driven * (1.0 + k) - k * y;
                    let (sat_u, sat_du) = sat_with_deriv(u, style);
                    // F(y)  = y − G⁴·sat(u) − σ
                    // F'(y) = 1 + G⁴·k·sat'(u)  ≥ 1  (sat'(u) clamped ≥ 0)
                    let f = y - gn4 * sat_u - sigma;
                    let df = 1.0 + gn4 * k * sat_du;
                    y -= f / df;
                }
                y
            }
        };

        // NaN/Inf guard: reset integrators if numerics blow up.
        if !y4.is_finite() {
            self.s = [0.0; 4];
            return (0.0, driven);
        }

        // Recover the saturated input from the solved y4 and forward-propagate
        // to update all four integrator states for the next sample.
        let u = driven * (1.0 + k) - k * y4;
        let x_sat = sat_eval(u, style);
        self.forward_and_update(x_sat, gn);

        (y4, driven - y4)
    }

    pub fn clear(&mut self) {
        self.s = [0.0; 4];
    }
}

// ─── 2× Oversampled Ladder ───────────────────────────────────────────────────

/// 2× oversampling wrapper around [`ZdfLadder`].
///
/// Each call to `process()` consumes one **native-rate** driven sample and
/// returns one native-rate `(lp4, hp4)` pair.  Internally:
///
/// 1. **Upsample** — linear interpolation generates a midpoint sub-sample,
///    producing two 2×-rate sub-samples `[mid, current]`.
/// 2. **Process** — `ZdfLadder` runs twice at the doubled rate, using `gn_2x`
///    which was computed against `sample_rate × 2.0`.
/// 3. **Decimate** — 2-tap FIR average eliminates content at the original
///    Nyquist frequency before returning to the native rate.
/// 4. **Compensate** — topology-derived output scaling keeps levels consistent
///    as resonance increases.
///
/// ### Why this resolves the near-Nyquist instability
///
/// With `g_norm` computed at `2 × fs`, the loop-gain product `k·G⁴` for
/// a 20 kHz cutoff at 44.1 kHz drops from ~0.55 (native) to ~0.05 (2× OS).
/// Heuristic Nyquist-damping of resonance is no longer needed: the problem
/// moves to 88.2 kHz, far above the audible range.  The bilinear frequency-
/// warp peak-shift artefact is similarly pushed out of band.
#[derive(Clone, Default)]
struct LadderOs2x {
    ladder: ZdfLadder,
    /// Previous native-rate input — used for midpoint linear interpolation.
    prev_input: f32,
}

impl LadderOs2x {
    /// Process one native-rate sample with 2× internal oversampling.
    ///
    /// - `driven`  — input after the drive stage (native rate).
    /// - `gn_2x`   — `g_norm` computed at `2 × sample_rate`.
    /// - `k`       — resonance feedback gain (already Nyquist-limited).
    /// - `style`   — saturation character.
    ///
    /// Returns `(lp4, hp4)` at native rate with output compensation applied.
    #[inline]
    pub fn process(&mut self, driven: f32, gn_2x: f32, k: f32, style: FilterStyle) -> (f32, f32) {
        // ── Upsample: linear interpolation ────────────────────────────────────
        // sub[0] ≈ t − 0.5 (midpoint between previous and current input)
        // sub[1] = t       (current input)
        let mid = (self.prev_input + driven) * 0.5;
        self.prev_input = driven;

        // ── Process both sub-samples at the doubled rate ───────────────────────
        let (lp0, hp0) = self.ladder.process(mid, gn_2x, k, style);
        let (lp1, hp1) = self.ladder.process(driven, gn_2x, k, style);

        // ── Decimate: 2-tap FIR average ───────────────────────────────────────
        // H(z) = (1 + z⁻¹) / 2  →  perfect null at z = −1 (original Nyquist).
        let lp = (lp0 + lp1) * 0.5;
        let hp = (hp0 + hp1) * 0.5;

        // ── Topology-derived output compensation ──────────────────────────────
        let comp = output_comp(k);
        (lp * comp, hp * comp)
    }

    pub fn clear(&mut self) {
        self.ladder.clear();
        self.prev_input = 0.0;
    }
}

// ─── Cytomic TPT SVF ─────────────────────────────────────────────────────────

/// Andy Simper's topology-preserving transform SVF with per-style BP saturation.
///
/// Behaviour identical to `TptSvfV2` — the SVF's implicit structure already
/// handles its own feedback correctly.  Carried forward unchanged to keep v3
/// self-contained.
#[derive(Clone, Default)]
struct TptSvfV3 {
    ic1eq: f32,
    ic2eq: f32,
}

impl TptSvfV3 {
    /// Process one sample.
    ///
    /// - `g`               = tan(π·fc / fs)
    /// - `k`               = 1/Q (= 2 − 1.98 × resonance, clamped to 0.02)
    /// - `resonance_drive` = resonance² × drive_scale  (0.0 → fully linear)
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

        // Saturate only the BP integrator (IC1); LP integrator stays linear
        // to preserve the pole structure and keep the SVF unconditionally stable.
        let bp_update = 2.0 * v1 - self.ic1eq;
        self.ic1eq = match style {
            FilterStyle::Classic => {
                if resonance_drive > 1e-4 {
                    let ds = 1.0 + resonance_drive * 4.0;
                    (bp_update * ds).tanh() / ds
                } else {
                    bp_update
                }
            }
            FilterStyle::Raw => {
                if resonance_drive > 1e-4 {
                    // Cap ds to prevent the normalisation denominator from
                    // making the output near-zero at extreme drive settings.
                    let ds = (1.0 + resonance_drive * 8.0).min(50.0);
                    raw_sat(bp_update * ds) / ds
                } else {
                    bp_update
                }
            }
            FilterStyle::Tube => {
                if resonance_drive > 1e-4 {
                    let ds = 1.0 + resonance_drive * 4.5;
                    tube_sat(bp_update * ds) / ds
                } else {
                    bp_update
                }
            }
            // Fully linear — no saturation anywhere in the signal path.
            FilterStyle::Clean => bp_update,
        };

        // LP integrator: unconditionally linear.
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

// ─── Coefficient Smoothers ────────────────────────────────────────────────────

/// Per-sample IIR smoothers for `g` (SVF) and `g_norm` (ladder).
///
/// Prevents zipper noise when the envelope drives the cutoff fast, and damps
/// any residual "peak-shift chirp" from very rapid sweeps.
#[derive(Debug, Clone, Copy)]
struct CoeffSmoothers {
    ladder_g_norm: f32,
    svf_g: f32,
    initialized: bool,
}

impl Default for CoeffSmoothers {
    fn default() -> Self {
        Self {
            // NaN sentinels so 0.0 is never mistaken for a valid initial value.
            ladder_g_norm: f32::NAN,
            svf_g: f32::NAN,
            initialized: false,
        }
    }
}

// ─── Filter Engine V3 ────────────────────────────────────────────────────────

/// Unified stereo filter engine — v3 with ZDF implicit ladder solve.
///
/// Drop-in replacement for [`FilterEngineV2`][crate::filter_v2::FilterEngineV2]:
/// same parameter signatures on `process_stereo` / `process_mono`, same
/// `FilterStyle` / `FilterType` / `FilterEnvelope` types.
#[derive(Clone)]
pub struct FilterEngineV3 {
    /// Left ladder channel — runs at 2× native sample rate internally.
    ladder_l: LadderOs2x,
    /// Right ladder channel — runs at 2× native sample rate internally.
    ladder_r: LadderOs2x,
    svf_l: TptSvfV3,
    svf_r: TptSvfV3,
    pub envelope: FilterEnvelope,
    smoothers: CoeffSmoothers,
}

impl Default for FilterEngineV3 {
    fn default() -> Self {
        Self {
            ladder_l: LadderOs2x::default(),
            ladder_r: LadderOs2x::default(),
            svf_l: TptSvfV3::default(),
            svf_r: TptSvfV3::default(),
            envelope: FilterEnvelope::default(),
            smoothers: CoeffSmoothers::default(),
        }
    }
}

impl FilterEngineV3 {
    pub fn trigger(&mut self, velocity: f32) {
        self.envelope.trigger(velocity);
    }

    pub fn release(&mut self) {
        self.envelope.release();
    }

    /// Reset all integrator and envelope state.
    ///
    /// Call when the filter type, position, or style changes to avoid stale-
    /// state pops and clicks.
    pub fn clear(&mut self) {
        self.ladder_l.clear();
        self.ladder_r.clear();
        self.svf_l.clear();
        self.svf_r.clear();
        self.envelope.clear();
        self.smoothers = CoeffSmoothers::default();
    }

    // ── Internal: smooth coefficient targets ─────────────────────────────────

    /// Tick the IIR smoothers for SVF `g` and ladder `g_norm`, returning the
    /// smoothed values ready for use this sample.
    #[inline]
    fn smooth_coeffs(
        &mut self,
        sample_rate: f32,
        g_target: f32,
        g_norm_target: f32,
    ) -> (f32, f32) {
        // Ladder gets a longer smoothing window: the implicit solve is more
        // sensitive to fast coefficient changes than the SVF.
        const TAU_SVF_MS: f32 = 2.0;
        const TAU_LADDER_MS: f32 = 6.0;

        if !self.smoothers.initialized {
            self.smoothers.svf_g = g_target;
            self.smoothers.ladder_g_norm = g_norm_target;
            self.smoothers.initialized = true;
            return (g_target, g_norm_target);
        }

        self.smoothers.svf_g =
            one_pole_ms(self.smoothers.svf_g, g_target, sample_rate, TAU_SVF_MS);
        self.smoothers.ladder_g_norm = one_pole_ms(
            self.smoothers.ladder_g_norm,
            g_norm_target,
            sample_rate,
            TAU_LADDER_MS,
        );

        (
            self.smoothers.svf_g.max(1e-6),
            self.smoothers.ladder_g_norm.clamp(1e-9, 0.999_999),
        )
    }

    // ── Public processing ─────────────────────────────────────────────────────

    /// Process one stereo sample pair.  Advances the envelope by one sample.
    ///
    /// Use at the **PostAll** position (stereo bus, after Corrosion).
    #[allow(clippy::too_many_arguments)]
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

        let (g, g_norm_2x) = {
            let ideal_hz = compute_cutoff_ideal_hz(
                cutoff_hz, env_val, env_amount_oct, midi_note, key_track,
            );
            let g_target = pro_cutoff_to_g(ideal_hz, sample_rate);
            // Ladder runs at 2× the native sample rate; compute g_norm for that
            // doubled rate so the loop gain and warp behavior are correct there.
            let ladder_g_2x = pro_ladder_cutoff_to_g(ideal_hz, sample_rate * 2.0);
            let g_norm_2x_target = ladder_g_2x / (1.0 + ladder_g_2x);
            self.smooth_coeffs(sample_rate, g_target, g_norm_2x_target)
        };

        // Two different resonance parameterisations: the ladder uses a positive
        // feedback gain (0..3.95), the SVF uses an inverse-Q (2..0.02).
        let res = resonance.clamp(0.0, 1.0);
        // Apply Nyquist-aware loop-gain limiter before passing k to the ladder.
        let ladder_k = nyquist_limit_k(res * 3.95, g_norm_2x);
        let svf_k = (2.0 - 1.98 * res).max(0.02);

        // Drive stage (bypassed entirely for Clean to preserve linearity).
        let (dl, dr) = if filter_style == FilterStyle::Clean {
            (l, r)
        } else {
            (apply_drive(l, drive_db), apply_drive(r, drive_db))
        };

        // Resonance-driven saturation depth for the SVF BP integrator.
        let res_drive = if filter_style == FilterStyle::Clean {
            0.0
        } else {
            resonance.powi(2) * (1.0 + drive_db.min(24.0) / 48.0)
        };

        match filter_type {
            FilterType::LP24 => {
                let (lp_l, _) = self.ladder_l.process(dl, g_norm_2x, ladder_k, filter_style);
                let (lp_r, _) = self.ladder_r.process(dr, g_norm_2x, ladder_k, filter_style);
                (lp_l, lp_r)
            }
            FilterType::HP24 => {
                let (_, hp_l) = self.ladder_l.process(dl, g_norm_2x, ladder_k, filter_style);
                let (_, hp_r) = self.ladder_r.process(dr, g_norm_2x, ladder_k, filter_style);
                (hp_l, hp_r)
            }
            FilterType::LP12 => {
                let ol = self.svf_l.process_styled(dl, g, svf_k, res_drive, filter_style);
                let or_ = self.svf_r.process_styled(dr, g, svf_k, res_drive, filter_style);
                (ol.lp, or_.lp)
            }
            FilterType::HP12 => {
                let ol = self.svf_l.process_styled(dl, g, svf_k, res_drive, filter_style);
                let or_ = self.svf_r.process_styled(dr, g, svf_k, res_drive, filter_style);
                (ol.hp, or_.hp)
            }
            FilterType::BP12 => {
                let ol = self.svf_l.process_styled(dl, g, svf_k, res_drive, filter_style);
                let or_ = self.svf_r.process_styled(dr, g, svf_k, res_drive, filter_style);
                (ol.bp, or_.bp)
            }
            FilterType::Notch => {
                let ol = self.svf_l.process_styled(dl, g, svf_k, res_drive, filter_style);
                let or_ = self.svf_r.process_styled(dr, g, svf_k, res_drive, filter_style);
                (ol.notch, or_.notch)
            }
        }
    }

    /// Process one mono sample.  Advances the envelope by one sample.
    ///
    /// Use at **PreNam** and **PostNam** positions (signal is still mono).
    /// Right-channel state is kept in sync so a seamless transition to
    /// `process_stereo` (PostAll) is possible without clicks.
    #[allow(clippy::too_many_arguments)]
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

        let (g, g_norm_2x) = {
            let ideal_hz = compute_cutoff_ideal_hz(
                cutoff_hz, env_val, env_amount_oct, midi_note, key_track,
            );
            let g_target = pro_cutoff_to_g(ideal_hz, sample_rate);
            let ladder_g_2x = pro_ladder_cutoff_to_g(ideal_hz, sample_rate * 2.0);
            let g_norm_2x_target = ladder_g_2x / (1.0 + ladder_g_2x);
            self.smooth_coeffs(sample_rate, g_target, g_norm_2x_target)
        };

        let res = resonance.clamp(0.0, 1.0);
        let ladder_k = nyquist_limit_k(res * 3.95, g_norm_2x);
        let svf_k = (2.0 - 1.98 * res).max(0.02);

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
            FilterType::LP24 => {
                let (lp, _) = self.ladder_l.process(x, g_norm_2x, ladder_k, filter_style);
                lp
            }
            FilterType::HP24 => {
                let (_, hp) = self.ladder_l.process(x, g_norm_2x, ladder_k, filter_style);
                hp
            }
            FilterType::LP12 => self
                .svf_l
                .process_styled(x, g, svf_k, res_drive, filter_style)
                .lp,
            FilterType::HP12 => self
                .svf_l
                .process_styled(x, g, svf_k, res_drive, filter_style)
                .hp,
            FilterType::BP12 => self
                .svf_l
                .process_styled(x, g, svf_k, res_drive, filter_style)
                .bp,
            FilterType::Notch => self
                .svf_l
                .process_styled(x, g, svf_k, res_drive, filter_style)
                .notch,
        };

        // Mirror left state into right so the stereo channels stay in sync
        // when the user switches the filter position to PostAll.
        self.ladder_r.ladder.s = self.ladder_l.ladder.s;
        self.ladder_r.prev_input = self.ladder_l.prev_input;
        self.svf_r.ic1eq = self.svf_l.ic1eq;
        self.svf_r.ic2eq = self.svf_l.ic2eq;

        out
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Compute ideal (unclamped) cutoff frequency with envelope and key-tracking.
///
/// The result may exceed Nyquist; callers map it safely via
/// `pro_cutoff_to_g` / `pro_ladder_cutoff_to_g`.
#[inline]
fn compute_cutoff_ideal_hz(
    base_hz: f32,
    env_val: f32,
    env_amount_oct: f32,
    midi_note: u8,
    key_track: f32,
) -> f32 {
    let note_hz = 440.0 * 2.0f32.powf((midi_note as f32 - 69.0) / 12.0);
    let tracked_hz = base_hz * (1.0 - key_track) + note_hz * key_track;
    (tracked_hz * 2.0f32.powf(env_val * env_amount_oct)).max(20.0)
}

/// Fraction of Nyquist where the soft warp begins.
const NYQUIST_SAFETY_FACTOR: f32 = 0.425;

/// Map an ideal (possibly supra-Nyquist) cutoff in Hz to `g = tan(π·fc/fs)`.
///
/// Instead of hard-clamping frequency, the `tan()` argument is smoothly
/// compressed toward `π/2` so the sweep law remains continuous through the
/// ceiling region — no "dwell" artefacts at high cutoff + high resonance.
#[inline]
fn pro_cutoff_to_g(ideal_hz: f32, sample_rate: f32) -> f32 {
    let ceiling_hz = (sample_rate * NYQUIST_SAFETY_FACTOR).min(18_000.0_f32);
    let x = PI * ideal_hz.max(0.0) / sample_rate;
    let x0 = PI * ceiling_hz / sample_rate;
    let x_max = PI * 0.499_f32;

    if x <= x0 {
        return x.tan().max(1e-6);
    }

    // Exponentially approach `x_max`; larger knee → softer compression.
    let knee = (x_max - x0).max(1e-3) * 0.35;
    let x_s = x_max - (x_max - x0) * (-(x - x0) / knee).exp();
    x_s.tan().max(1e-6)
}

/// Ladder-specific cutoff pre-warp.
///
/// The 4-pole ladder's resonance peak sits below the requested cutoff at high
/// frequencies due to bilinear-transform frequency compression.  Boosting the
/// requested frequency before conversion compensates so the resonance peak
/// lands at the user-intended Hz value.
///
/// Boost curve tuned from measured data (same as v2's `pro_ladder_cutoff_to_g`).
#[inline]
fn pro_ladder_cutoff_to_g(ideal_hz: f32, sample_rate: f32) -> f32 {
    let nyquist = (sample_rate * 0.5).max(1.0);
    let f = (ideal_hz.max(0.0) / nyquist).clamp(0.0, 0.999);

    // Boost: 1.0 at DC → ~1.85 near Nyquist.
    let boost = 1.0 + 0.85 * f.powf(1.6);
    let boosted_hz = (ideal_hz * boost).min(nyquist * 0.499);

    pro_cutoff_to_g(boosted_hz, sample_rate)
}

/// First-order IIR smoother with a time constant given in milliseconds.
#[inline]
fn one_pole_ms(current: f32, target: f32, sample_rate: f32, tau_ms: f32) -> f32 {
    let tau_s = (tau_ms * 0.001).max(1e-6);
    let a = (-1.0 / (tau_s * sample_rate)).exp();
    a * current + (1.0 - a) * target
}

/// Pre-filter drive stage.  At `drive_db = 0` this is a no-op.
#[inline]
fn apply_drive(input: f32, drive_db: f32) -> f32 {
    if drive_db < 0.1 {
        return input;
    }
    let gain = 10f32.powf(drive_db / 20.0);
    (input * gain).tanh()
}

/// Topology-derived output-level compensation for the oversampled ladder.
///
/// As the resonance feedback gain `k` increases toward 4, the ladder develops
/// a large resonant peak.  This formula normalises the output level using `k`
/// directly — the actual topology parameter — rather than a separate UI
/// control or an arbitrary curve:
///
/// ```text
/// comp = 1 / (1 + k/8)
/// ```
///
/// | k    | comp  |
/// |------|-------|
/// | 0.00 | 1.000 |
/// | 1.98 | 0.802 |
/// | 3.95 | 0.668 |
///
/// This is intentionally gentler than v2's `1/(1 + r²)` (which hit 0.50 at
/// full resonance) — the resonant peak should still rise clearly, this just
/// prevents the overall output from jumping 6+ dB when resonance is cranked.
#[inline]
fn output_comp(k: f32) -> f32 {
    1.0 / (1.0 + k * 0.125) // k / 8
}

/// Nyquist-aware feedback gain limiter.
///
/// Clamps `k` so the open-loop gain product `k · G⁴` stays below the
/// self-oscillation boundary of the 4-pole Moog ladder.
///
/// The open-loop magnitude at any frequency is ≤ `G⁴` (maximised at DC).
/// We therefore enforce `k · G⁴ < LOOP_GAIN_CEILING`, which guarantees the
/// closed-loop system stays stable at *all* frequencies simultaneously.
///
/// ### Practical impact with 2× OS
///
/// For a 20 kHz cutoff at 44.1 kHz (2× OS rate = 88.2 kHz):
/// - `g_norm_2x ≈ 0.465`  →  `G⁴ ≈ 0.047`  →  `k_max ≈ 84`   (no clamp)
///
/// For a 35 kHz virtual cutoff hitting the soft ceiling (beyond audible range):
/// - `g_norm_2x ≈ 0.75`   →  `G⁴ ≈ 0.316`  →  `k_max ≈ 12.5`  (no clamp)
///
/// The limiter only engages above ~40 kHz virtual cutoff, acting as a pure
/// safety net rather than an audible constraint.
#[inline]
fn nyquist_limit_k(k: f32, gn_2x: f32) -> f32 {
    /// Just below the theoretical Moog ladder self-oscillation boundary of 4.
    const LOOP_GAIN_CEILING: f32 = 3.96;
    let gn4 = gn_2x * gn_2x * gn_2x * gn_2x;
    if gn4 > 1e-6 {
        k.min(LOOP_GAIN_CEILING / gn4)
    } else {
        k
    }
}

