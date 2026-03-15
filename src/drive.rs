/*
 * Copyright (C) 2026 Marinus Burger
 */

//! Drive and saturation algorithms.
//!
//! All functions are stateless — they take a drive amount (0–1) and an input
//! sample and return the saturated output.  Output level compensation factors
//! are applied at the call site in `voice.rs`.

/// Tape-style saturation with a smooth exponential knee.
///
/// Adds classic analog "warmth": gentle compression in the linear region,
/// hard limit above 0.5 with an asymptotic approach to the ceiling.
#[inline(always)]
pub fn tape_classic(drive: f32, signal: f32) -> f32 {
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

/// Modern tape emulation with asymmetric soft-knee saturation.
///
/// Positive and negative halves saturate at slightly different rates,
/// which introduces subtle even-order harmonics characteristic of
/// transformer and tape head asymmetry.
#[inline(always)]
pub fn tape_modern(drive: f32, signal: f32) -> f32 {
    let gain = 1.0 + drive * 10.0;
    let x = signal * gain;
    let saturated = if x >= 0.0 {
        x / (1.0 + x.abs().powf(1.4))
    } else {
        x / (1.0 + (x.abs() * 0.85).powf(1.2))
    };
    saturated * 0.9
}

/// Triode tube saturation: linear below a bias point, tanh above.
///
/// The triode model has a sharp onset at 0.1 (the "grid bias") followed by
/// tanh saturation — mimicking a single tube stage operating class A.
#[inline(always)]
pub fn tube_triode(drive: f32, signal: f32) -> f32 {
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

/// Pentode tube saturation: wider linear region, atan ceiling.
///
/// Pentodes allow higher signal levels before saturation, then compress
/// hard using an atan characteristic — typical of push-pull output stages.
#[inline(always)]
pub fn tube_pentode(drive: f32, signal: f32) -> f32 {
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

/// Hard digital clipping with a polynomial soft-knee.
///
/// Uses a cubic soft-clipper below 1.0 followed by a rational limiter above,
/// which mimics over-driven A/D converters or bit-crushed material.
#[inline(always)]
pub fn saturation_digital(drive: f32, signal: f32) -> f32 {
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
