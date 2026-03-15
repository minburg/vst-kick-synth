/*
 * Copyright (C) 2026 Marinus Burger
 */

//! UI panel builder functions — one function per logical section of the editor.
//!
//! Each `build_*` function is responsible for a distinct visual zone and only
//! touches parameters relevant to that zone, making it easy to add, remove, or
//! restyle a section without disturbing others.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::ResizeHandle;

use crate::editor::data::{Data, PresetEvent};
use crate::editor::my_peak_meter::MyPeakMeter;
use crate::editor::single_knob::SingleKnob;
use crate::editor::vertical_param_slider::VerticalParamSlider;
use crate::editor::widgets::{create_text_button, create_toggle_button};
use crate::params::KickParams;

/// Gain (linear amplitude) → dB conversion used by the VU meters.
#[inline(always)]
fn gain_to_db(gain: f32) -> f32 {
    nih_plug::util::gain_to_db(gain)
}

// ── Source zone ─────────────────────────────────────────────────────────────────

/// Five-knob pentagon layout for the pitch core section.
///
/// Centre = Tune (large), corners = Waveform, Instability, Sweep, Pitch Decay.
pub fn build_pitch_core_pentagon(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        // LAYER 1: The Label
        Label::new(cx, "Core")
            .top(Stretch(0.1))
            .bottom(Stretch(0.9))
            .left(Stretch(0.5))
            .right(Stretch(0.5))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        // LAYER 2: The Grid (4 Corners)
        VStack::new(cx, |cx| {
            // TOP ROW: Waveform (Top-Left) and Instability (Top-Right)
            HStack::new(cx, |cx| {
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.waveform,
                    false,
                    85.0,
                    "vintage-knob",
                );

                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.analog_variation,
                    false,
                    85.0,
                    "vintage-knob",
                );
            });

            Element::new(cx).height(Stretch(1.0));

            // BOTTOM ROW: Sweep (Bottom-Left) and Pitch Decay (Bottom-Right)
            HStack::new(cx, |cx| {
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.sweep,
                    false,
                    85.0,
                    "vintage-knob",
                );

                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.pitch_decay,
                    false,
                    85.0,
                    "vintage-knob",
                );
            });
        })
        .height(Stretch(1.0))
        .class("orange");

        // LAYER 3: Centre Tune knob (overlaid)
        SingleKnob::new(
            cx,
            Data::params,
            |p| &p.tune,
            false,
            130.0,
            "vintage-knob",
        )
        .class("large-center-knob")
        .top(Stretch(1.0))
        .bottom(Stretch(1.0))
        .left(Stretch(1.0))
        .right(Stretch(1.0));
    })
    .top(Stretch(0.04))
    .bottom(Stretch(0.04))
    .left(Stretch(0.04))
    .right(Stretch(0.04));
}

/// Diamond (three-row) layout for the pitch core — alternate presentation.
/// Not currently wired into the main layout but kept for future use.
#[allow(dead_code)]
pub fn build_pitch_core_diamond(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        Label::new(cx, "Core")
            .top(Stretch(1.0))
            .bottom(Stretch(1.0))
            .left(Stretch(1.0))
            .right(Stretch(1.0))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        VStack::new(cx, |cx| {
            HStack::new(cx, |cx| {
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(cx, Data::params, |p| &p.tune, false, 85.0, "vintage-knob");
                Element::new(cx).width(Stretch(1.0));
            });

            HStack::new(cx, |cx| {
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.analog_variation,
                    false,
                    85.0,
                    "vintage-knob",
                );
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.pitch_decay,
                    false,
                    85.0,
                    "vintage-knob",
                );
            });

            HStack::new(cx, |cx| {
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(cx, Data::params, |p| &p.sweep, false, 85.0, "vintage-knob");
                Element::new(cx).width(Stretch(1.0));
            });
        })
        .class("red");
    })
    .top(Stretch(0.08))
    .bottom(Stretch(0.08))
    .left(Stretch(0.08))
    .right(Stretch(0.08));
}

/// Five-knob pentagon for the texture section.
///
/// Centre = Texture Amount (large), corners = Type, Variation, Tone, Decay.
pub fn build_texture_pentagon(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        Label::new(cx, "Texture")
            .top(Stretch(0.1))
            .bottom(Stretch(0.9))
            .left(Stretch(0.5))
            .right(Stretch(0.5))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        VStack::new(cx, |cx| {
            HStack::new(cx, |cx| {
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.tex_type,
                    false,
                    85.0,
                    "vintage-knob",
                );

                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.tex_variation,
                    false,
                    85.0,
                    "vintage-knob",
                );
            });

            Element::new(cx).height(Stretch(1.0));

            HStack::new(cx, |cx| {
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.tex_tone,
                    false,
                    85.0,
                    "vintage-knob",
                );

                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.tex_decay,
                    false,
                    85.0,
                    "vintage-knob",
                );
            });
        })
        .height(Stretch(1.0))
        .class("orange");

        SingleKnob::new(
            cx,
            Data::params,
            |p| &p.tex_amt,
            false,
            130.0,
            "vintage-knob",
        )
        .class("large-center-knob")
        .top(Stretch(1.0))
        .bottom(Stretch(1.0))
        .left(Stretch(1.0))
        .right(Stretch(1.0));
    })
    .top(Stretch(0.04))
    .bottom(Stretch(0.04))
    .left(Stretch(0.04))
    .right(Stretch(0.04));
}

// ── Body zone ───────────────────────────────────────────────────────────────────

/// Centre column: preset header, version/links, amplitude ADSR faders, and
/// mode/trigger controls.
pub fn build_center_amp_env(params: &Arc<KickParams>, cx: &mut Context) {
    ZStack::new(cx, |cx| {
        VStack::new(cx, |cx| {
            VStack::new(cx, |cx| {
                build_preset_header(cx);

                Label::new(cx, "v0.2.2")
                    .class("header-version-title")
                    .height(Stretch(0.5))
                    .width(Stretch(0.2))
                    .left(Stretch(0.2))
                    .right(Stretch(0.2))
                    .child_space(Stretch(1.0));

                Label::new(cx, "Check for Updates")
                    .class("update-link")
                    .on_press(|_| {
                        if let Err(e) = webbrowser::open(
                            "https://github.com/minburg/vst-kick-synth/releases",
                        ) {
                            nih_log!("Failed to open browser: {}", e);
                        }
                    })
                    .height(Stretch(1.0))
                    .left(Stretch(1.0))
                    .right(Stretch(1.0))
                    .child_space(Stretch(1.0));

                HStack::new(cx, |cx| {
                    Element::new(cx)
                        .class("insta-button")
                        .on_press(|_| {
                            let _ = webbrowser::open(
                                "https://www.instagram.com/convolution.official/",
                            );
                        });
                    Element::new(cx)
                        .class("spotify-button")
                        .opacity(0.5)
                        .on_press(|_| {
                            let _ = webbrowser::open(
                                "https://open.spotify.com/artist/7k0eMwQbplT3Zyyy0DalRL?si=aalp-7GQQ2O_cZRodAlsNg",
                            );
                        });
                })
                .height(Stretch(1.0))
                .width(Stretch(1.0))
                .child_space(Stretch(1.0))
                .child_top(Stretch(0.01))
                .child_bottom(Stretch(0.01))
                .class("link-section");
            })
            .row_between(Pixels(15.0))
            .height(Stretch(0.6));

            HStack::new(cx, |cx| {
                VStack::new(cx, |cx| {
                    VerticalParamSlider::new(cx, Data::params, |p| &p.attack)
                        .height(Stretch(1.0))
                        .width(Stretch(0.5));
                    Label::new(cx, "[A]")
                        .height(Stretch(0.2))
                        .left(Stretch(1.0))
                        .right(Stretch(1.0))
                        .child_space(Stretch(1.0))
                        .class("adsr-label");
                })
                .row_between(Pixels(8.0));

                VStack::new(cx, |cx| {
                    VerticalParamSlider::new(cx, Data::params, |p| &p.decay)
                        .height(Stretch(1.0))
                        .width(Stretch(0.5));
                    Label::new(cx, "[D]")
                        .height(Stretch(0.2))
                        .left(Stretch(1.0))
                        .right(Stretch(1.0))
                        .child_space(Stretch(1.0))
                        .class("adsr-label");
                })
                .row_between(Pixels(8.0));

                VStack::new(cx, |cx| {
                    VerticalParamSlider::new(cx, Data::params, |p| &p.sustain)
                        .height(Stretch(1.0))
                        .width(Stretch(0.5));
                    Label::new(cx, "[S]")
                        .height(Stretch(0.2))
                        .left(Stretch(1.0))
                        .right(Stretch(1.0))
                        .child_space(Stretch(1.0))
                        .class("adsr-label");
                })
                .row_between(Pixels(8.0));

                VStack::new(cx, |cx| {
                    VerticalParamSlider::new(cx, Data::params, |p| &p.release)
                        .height(Stretch(1.0))
                        .width(Stretch(1.0));
                    Label::new(cx, "[R]")
                        .height(Stretch(0.2))
                        .left(Stretch(1.0))
                        .right(Stretch(1.0))
                        .child_space(Stretch(1.0))
                        .class("adsr-label");
                })
                .width(Stretch(1.0))
                .row_between(Pixels(8.0));
            })
            .child_space(Stretch(0.6))
            .child_top(Stretch(0.1))
            .child_bottom(Stretch(0.1))
            .col_between(Pixels(16.0))
            .height(Stretch(1.0))
            .width(Stretch(1.0));

            VStack::new(cx, |cx| {
                HStack::new(cx, |cx| {
                    create_toggle_button(
                        cx,
                        "808 Mode",
                        Data::params.map(|p| p.bass_synth_mode.value()),
                        params,
                        |p| &p.bass_synth_mode,
                        "switch-button",
                        "active",
                    )
                    .left(Stretch(0.3))
                    .right(Stretch(0.3))
                    .top(Stretch(0.05))
                    .bottom(Stretch(0.05))
                    .width(Stretch(1.0))
                    .height(Pixels(60.0))
                    .child_space(Stretch(1.0));
                    create_toggle_button(
                        cx,
                        "NAM",
                        Data::params.map(|p| p.nam_active.value()),
                        params,
                        |p| &p.nam_active,
                        "switch-button",
                        "active",
                    )
                    .left(Stretch(0.3))
                    .right(Stretch(0.3))
                    .top(Stretch(0.05))
                    .bottom(Stretch(0.05))
                    .width(Stretch(1.0))
                    .height(Pixels(60.0))
                    .child_space(Stretch(1.0));
                })
                .height(Pixels(60.0));

                MyPeakMeter::new(
                    cx,
                    Data::peak_meter_l
                        .map(|peak_meter_l| gain_to_db(peak_meter_l.load(Ordering::Relaxed))),
                    Some(Duration::from_millis(30)),
                    true,
                )
                .class("vu-meter-no-text")
                .top(Stretch(0.05))
                .bottom(Stretch(0.05))
                .width(Stretch(1.0))
                .height(Pixels(40.0));

                MyPeakMeter::new(
                    cx,
                    Data::peak_meter_r
                        .map(|peak_meter_r| gain_to_db(peak_meter_r.load(Ordering::Relaxed))),
                    Some(Duration::from_millis(30)),
                    false,
                )
                .class("vu-meter-no-text")
                .top(Stretch(0.05))
                .bottom(Stretch(0.05))
                .width(Stretch(1.0))
                .height(Pixels(40.0));

                // Trigger Button
                create_text_button(
                    cx,
                    "TRIGGER",
                    Data::params.map(|p| p.trigger.value()),
                    params,
                    |p| &p.trigger,
                    "distortion-param-button",
                    "active",
                )
                .left(Stretch(0.1))
                .right(Stretch(0.1))
                .top(Stretch(0.05))
                .bottom(Stretch(0.05))
                .width(Stretch(1.0))
                .height(Pixels(60.0))
                .child_space(Stretch(1.0));
            })
            .height(Stretch(1.0));
        });
    });
}

// ── Mangle zone ─────────────────────────────────────────────────────────────────

/// Two-knob pill layout for the drive section (model + gain).
pub fn build_drive_pill(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        Label::new(cx, "Drive")
            .top(Stretch(1.0))
            .bottom(Stretch(1.0))
            .left(Stretch(1.0))
            .right(Stretch(1.0))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        HStack::new(cx, |cx| {
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.drive_model,
                false,
                85.0,
                "vintage-knob",
            );

            SingleKnob::new(cx, Data::params, |p| &p.drive, false, 85.0, "vintage-knob");
        })
        .class("orange")
        .height(Stretch(0.5));
    })
    .top(Stretch(0.02))
    .bottom(Stretch(0.02))
    .left(Stretch(0.02))
    .right(Stretch(0.02));
}

/// Five-knob pentagon for the corrosion (wow/flutter) section.
///
/// Centre = Amount, corners = Frequency, Width, Noise Blend, Stereo.
pub fn build_corrosion_pentagon(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        Label::new(cx, "Corrosion")
            .top(Stretch(0.0))
            .bottom(Stretch(1.0))
            .left(Stretch(0.5))
            .right(Stretch(0.5))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        VStack::new(cx, |cx| {
            HStack::new(cx, |cx| {
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.corrosion_frequency,
                    false,
                    85.0,
                    "vintage-knob",
                );

                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.corrosion_width,
                    false,
                    85.0,
                    "vintage-knob",
                );
            });

            HStack::new(cx, |cx| {
                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.corrosion_amount,
                    false,
                    130.0,
                    "vintage-knob",
                )
                .class("large-center-knob");

                Element::new(cx).width(Stretch(1.0));
            });

            HStack::new(cx, |cx| {
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.corrosion_noise_blend,
                    false,
                    85.0,
                    "vintage-knob",
                );

                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.corrosion_stereo,
                    false,
                    85.0,
                    "vintage-knob",
                );
            });
        })
        .height(Stretch(1.0))
        .class("orange");
    })
    .top(Stretch(0.02))
    .bottom(Stretch(0.02))
    .left(Stretch(0.02))
    .right(Stretch(0.02));
}

/// Three-knob triangle for the NAM & output level section.
///
/// Top = NAM Model, Bottom-Left = NAM Input Gain, Bottom-Right = Master Out.
pub fn build_nam_triangle(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        Label::new(cx, "NAM & Out")
            .top(Stretch(0.7))
            .bottom(Stretch(0.3))
            .left(Stretch(1.0))
            .right(Stretch(1.0))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        Label::new(cx, Data::params.map(|p| p.nam_status_text.read().clone()))
            .class("nam-status-label")
            .toggle_class(
                "success",
                Data::params.map(|p| p.nam_is_loaded.load(Ordering::Relaxed)),
            )
            .toggle_class(
                "error",
                Data::params.map(|p| !p.nam_is_loaded.load(Ordering::Relaxed)),
            )
            .top(Stretch(0.85))
            .bottom(Stretch(0.15))
            .left(Stretch(1.0))
            .right(Stretch(1.0))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0));

        VStack::new(cx, |cx| {
            HStack::new(cx, |cx| {
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.nam_model,
                    false,
                    85.0,
                    "vintage-knob",
                );
                Element::new(cx).width(Stretch(1.0));
            });

            HStack::new(cx, |cx| {
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.nam_input_gain,
                    false,
                    85.0,
                    "vintage-knob",
                );
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(
                    cx,
                    Data::params,
                    |p| &p.output_gain,
                    false,
                    85.0,
                    "vintage-knob",
                );
            });
        })
        .class("orange")
        .height(Stretch(1.0));
    })
    .top(Stretch(0.02))
    .bottom(Stretch(0.02))
    .left(Stretch(0.02))
    .right(Stretch(0.02));
}

// ── Filter section ──────────────────────────────────────────────────────────────

/// Full filter strip: ON toggle, Type + Position, core knobs, and envelope ADSR.
pub fn build_filter_section(params: &Arc<KickParams>, cx: &mut Context) {
    HStack::new(cx, |cx| {
        // ── Label + ON/OFF toggle ─────────────────────────────────────────
        VStack::new(cx, |cx| {
            Label::new(cx, "Filter")
                .class("pentagon-label")
                .child_space(Stretch(1.0))
                .height(Stretch(1.0));

            Element::new(cx).height(Stretch(0.3));

            create_toggle_button(
                cx,
                "ON",
                Data::params.map(|p| p.filter_active.value()),
                params,
                |p| &p.filter_active,
                "filter-button",
                "active",
            )
            .height(Pixels(50.0))
            .width(Stretch(3.0))
            .child_space(Stretch(1.0));
        })
        .child_space(Stretch(1.0))
        .width(Stretch(0.22))
        .class("filter-ctrl-group");

        // ── Type + Position enum knobs ────────────────────────────────────
        HStack::new(cx, |cx| {
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_type,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
            Element::new(cx).width(Stretch(0.2));
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_position,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
        })
        .width(Stretch(0.5))
        .class("filter-ctrl-group");

        // ── Core params: Cutoff, Resonance, Trigger Mode, Key Track ──────────────
        HStack::new(cx, |cx| {
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_cutoff,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
            Element::new(cx).width(Stretch(0.2));
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_resonance,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
            Element::new(cx).width(Stretch(0.2));
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_env_trigger,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
            Element::new(cx).width(Stretch(0.2));
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_key_track,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
        })
        .width(Stretch(1.0))
        .class("filter-ctrl-group");

        // ── Filter Envelope: Amount, A, D, S, R ──────────────────────────
        HStack::new(cx, |cx| {
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_env_amount,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
            Element::new(cx).width(Stretch(0.2));
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_env_attack,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
            Element::new(cx).width(Stretch(0.2));
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_env_decay,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
            Element::new(cx).width(Stretch(0.2));
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_env_sustain,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
            Element::new(cx).width(Stretch(0.2));
            SingleKnob::new(
                cx,
                Data::params,
                |p| &p.filter_env_release,
                false,
                85.0,
                "vintage-knob-poti1",
            )
            .width(Stretch(1.0));
        })
        .width(Stretch(1.2))
        .class("filter-ctrl-group");
    })
    .col_between(Pixels(10.0))
    .width(Stretch(1.0))
    .height(Stretch(0.18))
    .class("filter-section");
}

// ── Header ──────────────────────────────────────────────────────────────────────

/// Preset picker row with Save / Load file buttons.
pub fn build_preset_header(cx: &mut Context) {
    HStack::new(cx, |cx| {
        Label::new(cx, "Preset:").class("preset-label");

        PickList::new(
            cx,
            Data::factory_presets
                .map(|p| p.iter().map(|pr| pr.name.clone()).collect::<Vec<_>>()),
            Data::selected_preset,
            true,
        )
        .on_select(|cx, index| cx.emit(PresetEvent::LoadFactory(index)))
        .width(Pixels(200.0))
        .class("preset-dropdown");

        Button::new(
            cx,
            |cx| cx.emit(PresetEvent::SaveToFile),
            |cx| Label::new(cx, "Save"),
        )
        .class("preset-button");

        Button::new(
            cx,
            |cx| cx.emit(PresetEvent::LoadFromFile),
            |cx| Label::new(cx, "Load"),
        )
        .class("preset-button");
    })
    .child_space(Stretch(1.0))
    .col_between(Pixels(10.0))
    .height(Pixels(40.0))
    .class("preset-header");
}

// ── Top-level layout ─────────────────────────────────────────────────────────────

/// Assemble the full editor layout from all zone panels.
pub fn build_main_layout(params: &Arc<KickParams>, cx: &mut Context) {
    VStack::new(cx, |cx| {
        VStack::new(cx, |cx| {
            VStack::new(cx, |cx| {
                Label::new(cx, "CONVOLUTION'S Kick Synth").class("header-title");
            })
            .width(Stretch(1.0))
            .height(Stretch(0.1))
            .row_between(Pixels(10.0))
            .child_space(Stretch(1.0))
            .class("title-section");

            HStack::new(cx, |cx| {
                VStack::new(cx, |_cx| {})
                    .width(Stretch(0.1))
                    .class("zone-source");

                // ZONE 1: THE SOURCE (Generators)
                VStack::new(cx, |cx| {
                    build_pitch_core_pentagon(cx);
                    build_texture_pentagon(cx);
                })
                .width(Stretch(1.2))
                .class("zone-source");

                // ZONE 2: THE BODY (Amp Envelope)
                VStack::new(cx, |cx| {
                    build_center_amp_env(params, cx);
                })
                .width(Stretch(1.0))
                .class("zone-body");

                // ZONE 3: THE MANGLE (Destruction)
                VStack::new(cx, |cx| {
                    build_drive_pill(cx);
                    build_corrosion_pentagon(cx);
                    build_nam_triangle(cx);
                })
                .width(Stretch(1.2))
                .class("zone-mangle");

                VStack::new(cx, |_cx| {})
                    .width(Stretch(0.1))
                    .class("zone-source");
            })
            .width(Stretch(1.0))
            .height(Stretch(0.73));

            // ── FILTER SECTION ────────────────────────────────────────────
            build_filter_section(params, cx);
        })
        .width(Stretch(1.0))
        .height(Stretch(1.0))
        .class("main-gui-transparent");
    })
    .width(Stretch(1.0))
    .height(Stretch(1.0))
    .class("main-gui");

    ResizeHandle::new(cx);
}
