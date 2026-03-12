use crate::editor::single_knob::{SingleKnob, SingleKnobExt};
use crate::util::gain_to_db;
use nih_plug::prelude::*;
use nih_plug_vizia::assets::register_noto_sans_light;
use nih_plug_vizia::vizia::image::load_from_memory;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::{ParamEvent, RawParamEvent, ResizeHandle};
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::editor::my_peak_meter::MyPeakMeter;
use crate::presets::{self, Preset};
use crate::KickParams;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

mod my_peak_meter;
mod param_knob;
mod single_knob;
mod vertical_param_slider;
use vertical_param_slider::VerticalParamSlider;
mod util;

pub const ORBITRON_TTF: &[u8] = include_bytes!("resource/fonts/Orbitron-Regular.ttf");
pub const COMFORTAA_LIGHT_TTF: &[u8] = include_bytes!("resource/fonts/Comfortaa-Light.ttf");
pub const COMFORTAA: &str = "Comfortaa";

const BG_IMAGE_BYTES: &[u8] = include_bytes!("resource/images/kick_background_tint_cropped.png");
const POTI_3_IMAGE_BYTES: &[u8] = include_bytes!("resource/images/poti_3_fixed_small.png");
const INSTA_ICON_BYTES: &[u8] = include_bytes!("resource/images/instagram_icon.png");
const SPOTIFY_ICON_BYTES: &[u8] = include_bytes!("resource/images/spotify_icon.png");

#[derive(Lens)]
struct Data {
    params: Arc<KickParams>,
    peak_meter_l: Arc<AtomicF32>,
    peak_meter_r: Arc<AtomicF32>,
    factory_presets: Vec<Preset>,
    selected_preset: usize,
}

impl Model for Data {
    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|preset_event, _| match preset_event {
            PresetEvent::LoadFactory(idx) => {
                let preset = &self.factory_presets[*idx];
                self.selected_preset = *idx;
                emit_params_events(cx, &self.params, preset);
            }
            PresetEvent::SaveToFile => {
                cx.spawn(|cxp| {
                    let path = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .save_file();
                    let _ = cxp.emit(PresetEvent::SaveToFileResult(path));
                });
            }
            PresetEvent::SaveToFileResult(path) => {
                if let Some(path) = path {
                    let preset = self.params.get_current_preset();
                    match serde_json::to_string_pretty(&preset) {
                        Ok(json) => {
                            if let Ok(mut file) = File::create(path) {
                                let _ = file.write_all(json.as_bytes());
                            }
                        }
                        Err(e) => nih_log!("Failed to serialize preset: {}", e),
                    }
                }
            }
            PresetEvent::LoadFromFile => {
                cx.spawn(|cxp| {
                    let path = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file();
                    let _ = cxp.emit(PresetEvent::LoadFromFileResult(path));
                });
            }
            PresetEvent::LoadFromFileResult(path) => {
                if let Some(path) = path {
                    match File::open(path) {
                        Ok(mut file) => {
                            let mut json = String::new();
                            if file.read_to_string(&mut json).is_ok() {
                                match serde_json::from_str::<Preset>(&json) {
                                    Ok(preset) => {
                                        emit_params_events(cx, &self.params, &preset);
                                    }
                                    Err(e) => nih_log!("Failed to deserialize preset: {}", e),
                                }
                            }
                        }
                        Err(e) => nih_log!("Failed to open preset file: {}", e),
                    }
                }
            }
        });
    }
}

pub enum PresetEvent {
    LoadFactory(usize),
    SaveToFile,
    SaveToFileResult(Option<PathBuf>),
    LoadFromFile,
    LoadFromFileResult(Option<PathBuf>),
}

fn emit_params_events(cx: &mut EventContext, params: &Arc<KickParams>, preset: &Preset) {
    // Helper to emit events for a single param
    fn emit<P: Param + Sync + 'static>(cx: &mut EventContext, param: &P, value: P::Plain)
    where
        P::Plain: Send + Clone,
    {
        let ptr = param.as_ptr();
        // transmuting to 'static is safe here because we know params outlives the editor
        let param_static: &'static P = unsafe { std::mem::transmute(param) };

        let normalized = param.preview_normalized(value.clone());

        cx.emit(ParamEvent::<P>::BeginSetParameter(param_static));
        cx.emit(RawParamEvent::BeginSetParameter(ptr));
        cx.emit(ParamEvent::<P>::SetParameter(param_static, value));
        cx.emit(RawParamEvent::SetParameterNormalized(ptr, normalized));
        cx.emit(ParamEvent::<P>::EndSetParameter(param_static));
        cx.emit(RawParamEvent::EndSetParameter(ptr));
    }

    emit(cx, &params.tune, preset.tune);
    emit(cx, &params.sweep, preset.sweep);
    emit(cx, &params.pitch_decay, preset.pitch_decay);
    emit(cx, &params.drive, preset.drive);
    emit(cx, &params.drive_model, preset.drive_model);
    emit(cx, &params.tex_amt, preset.tex_amt);
    emit(cx, &params.tex_decay, preset.tex_decay);
    emit(cx, &params.tex_variation, preset.tex_variation);
    emit(cx, &params.analog_variation, preset.analog_variation);
    emit(cx, &params.tex_type, preset.tex_type);
    emit(cx, &params.tex_tone, preset.tex_tone);
    emit(cx, &params.attack, preset.attack);
    emit(cx, &params.decay, preset.decay);
    emit(cx, &params.sustain, preset.sustain);
    emit(cx, &params.release, preset.release);
    emit(cx, &params.corrosion_frequency, preset.corrosion_frequency);
    emit(cx, &params.corrosion_width, preset.corrosion_width);
    emit(
        cx,
        &params.corrosion_noise_blend,
        preset.corrosion_noise_blend,
    );
    emit(cx, &params.corrosion_stereo, preset.corrosion_stereo);
    emit(cx, &params.corrosion_amount, preset.corrosion_amount);
    emit(cx, &params.bass_synth_mode, preset.bass_synth_mode);
    emit(cx, &params.nam_active, preset.nam_active);
    emit(cx, &params.nam_input_gain, preset.nam_input_gain);
    emit(cx, &params.nam_output_gain, preset.nam_output_gain);
    emit(cx, &params.nam_model, preset.nam_model);
}

pub(crate) fn default_state() -> Arc<ViziaState> {
    ViziaState::new(|| (1400, 950))
}

pub(crate) fn create(
    params: Arc<KickParams>,
    peak_meter_l: Arc<AtomicF32>,
    peak_meter_r: Arc<AtomicF32>,
    editor_state: Arc<ViziaState>,
) -> Option<Box<dyn Editor>> {
    create_vizia_editor(editor_state, ViziaTheming::Custom, move |cx, _| {
        register_noto_sans_light(cx);

        cx.add_font_mem(&COMFORTAA_LIGHT_TTF);
        cx.add_font_mem(&ORBITRON_TTF);
        cx.set_default_font(&[COMFORTAA]);

        match load_from_memory(BG_IMAGE_BYTES) {
            Ok(img) => cx.load_image("background_image.png", img, ImageRetentionPolicy::Forever),
            Err(e) => nih_error!("Failed to load image: {}", e),
        }

        match load_from_memory(POTI_3_IMAGE_BYTES) {
            Ok(img) => cx.load_image("poti_3_fixed_small.png", img, ImageRetentionPolicy::Forever),
            Err(e) => nih_error!("Failed to load image: {}", e),
        }

        match load_from_memory(INSTA_ICON_BYTES) {
            Ok(img) => cx.load_image("insta.png", img, ImageRetentionPolicy::Forever),
            Err(e) => nih_error!("Failed to load image: {}", e),
        }

        match load_from_memory(SPOTIFY_ICON_BYTES) {
            Ok(img) => cx.load_image("spotify.png", img, ImageRetentionPolicy::Forever),
            Err(e) => nih_error!("Failed to load image: {}", e),
        }

        if let Err(e) = cx.add_stylesheet(include_style!("/src/resource/style.css")) {
            nih_log!("CSS Error: {:?}", e);
        }

        Data {
            params: params.clone(),
            peak_meter_l: peak_meter_l.clone(),
            peak_meter_r: peak_meter_r.clone(),
            factory_presets: presets::get_factory_presets(),
            selected_preset: 0,
        }
        .build(cx);

        // --------------------------------------------------------------------------------------------------------------- UI

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
                    VStack::new(cx, |cx| {})
                        .width(Stretch(0.1))
                        .class("zone-source");

                    // ZONE 1: THE SOURCE (Generators)
                    VStack::new(cx, |cx| {
                        build_pitch_core_diamond(cx);
                        build_texture_pentagon(cx);
                    })
                    .width(Stretch(1.2))
                    .class("zone-source");

                    // ZONE 2: THE BODY (Amp Envelope)
                    VStack::new(cx, |cx| {
                        build_center_amp_env(&params, cx);
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

                    VStack::new(cx, |cx| {})
                        .width(Stretch(0.1))
                        .class("zone-source");
                })
                .width(Stretch(1.0))
                .height(Stretch(0.9));
            })
            .width(Stretch(1.0))
            .height(Stretch(1.0))
            .class("main-gui-transparent");
        })
        .width(Stretch(1.0))
        .height(Stretch(1.0))
        .class("main-gui");

        ResizeHandle::new(cx);
    })
}

fn build_pitch_core_diamond(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        // LAYER 1: The Label
        Label::new(cx, "Core")
            .top(Stretch(1.0))
            .bottom(Stretch(1.0))
            .left(Stretch(1.0))
            .right(Stretch(1.0))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        // Inside your UI build function or a helper:
        VStack::new(cx, |cx| {
            // TOP ROW: Tune (Centered)
            HStack::new(cx, |cx| {
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(cx, Data::params, |p| &p.tune, false, 85.0);
                Element::new(cx).width(Stretch(1.0));
            });

            // MIDDLE ROW: Drift & Decay (Pushed to edges)
            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |p| &p.analog_variation, false, 85.0);
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(cx, Data::params, |p| &p.pitch_decay, false, 85.0);
            });

            // BOTTOM ROW: Sweep (Centered)
            HStack::new(cx, |cx| {
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(cx, Data::params, |p| &p.sweep, false, 85.0);
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

fn build_texture_pentagon(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        // LAYER 1: The Label
        Label::new(cx, "Texture")
            .top(Stretch(0.1))
            .bottom(Stretch(0.9))
            .left(Stretch(0.5))
            .right(Stretch(0.5))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        // LAYER 2: The Grid (4 Corners)
        VStack::new(cx, |cx| {
            // TOP ROW: Tex Type (Top-Left) and Variation (Top-Right)
            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |p| &p.tex_type, false, 85.0);

                // This spring sits between the knobs, pushing them to the far left and right edges
                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |p| &p.tex_variation, false, 85.0);
            });

            // MIDDLE ROW: An empty vertical spring to push the top and bottom rows apart
            Element::new(cx).height(Stretch(1.0));

            // BOTTOM ROW: Tex Tone (Bottom-Left) and Tex Decay (Bottom-Right)
            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |p| &p.tex_tone, false, 85.0);

                // Another spring in the middle pushing these to the bottom corners
                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |p| &p.tex_decay, false, 85.0);
            });
        })
        .height(Stretch(1.0))
        .class("orange");

        // LAYER 3: The Giant Center Knob (Foreground)
        // By making it a direct child of the ZStack, it overlaps the VStack without expanding its rows
        SingleKnob::new(cx, Data::params, |p| &p.tex_amt, false, 180.0)
            .class("large-center-knob")
            // Apply equal springs to all sides to perfectly center it within the ZStack
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

fn build_center_amp_env(params: &Arc<KickParams>, cx: &mut Context) {
    ZStack::new(cx, |cx| {
        VStack::new(cx, |cx| {
            VStack::new(cx, |cx| {

                build_preset_header(cx);

                Label::new(cx, "v0.2.0").class("header-version-title")
                    .height(Stretch(0.5))
                    .width(Stretch(0.2))
                    .left(Stretch(0.2))
                    .right(Stretch(0.2))
                    .child_space(Stretch(1.0));

                Label::new(cx, "Check for Updates")
                    .class("update-link")
                    .on_press(|_| {
                        if let Err(e) = webbrowser::open("https://github.com/minburg/vst-kick-synth/releases") {
                            nih_log!("Failed to open browser: {}", e);
                        }
                    })
                    .height(Stretch(1.0))
                    .left(Stretch(1.0))
                    .right(Stretch(1.0))
                    // .width(Stretch(0.5))
                    .child_space(Stretch(1.0));


                HStack::new(cx, |cx| {

                    Element::new(cx)
                        .class("insta-button")
                        .on_press(|_| {
                            let _ = webbrowser::open("https://www.instagram.com/convolution.official/");
                        });
                    Element::new(cx)
                        .class("spotify-button").opacity(0.5)
                        .on_press(|_| {
                            let _ = webbrowser::open("https://open.spotify.com/artist/7k0eMwQbplT3Zyyy0DalRL?si=aalp-7GQQ2O_cZRodAlsNg");
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
                        &params,
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
                        &params,
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
                    &params,
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

fn build_drive_pill(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        // LAYER 1: The Label
        Label::new(cx, "Drive")
            .top(Stretch(1.0))
            .bottom(Stretch(1.0))
            .left(Stretch(1.0))
            .right(Stretch(1.0))
            .width(Stretch(0.5))
            .child_space(Stretch(1.0))
            .class("pentagon-label");

        HStack::new(cx, |cx| {
            SingleKnob::new(cx, Data::params, |p| &p.drive_model, false, 85.0);

            SingleKnob::new(cx, Data::params, |p| &p.drive, false, 85.0);
        })
        .class("orange")
        .height(Stretch(0.5));
    })
    .top(Stretch(0.02))
    .bottom(Stretch(0.02))
    .left(Stretch(0.02))
    .right(Stretch(0.02));
}

fn build_corrosion_pentagon(cx: &mut Context) {
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
            // TOP ROW: Frequency (Top-Left) and Width (Top-Right)
            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |p| &p.corrosion_frequency, false, 85.0);

                // This spring sits between the knobs, pushing them to the far left and right edges
                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |p| &p.corrosion_width, false, 85.0);
            });

            // MIDDLE ROW: The main Corrosion Amount (Center)
            HStack::new(cx, |cx| {
                Element::new(cx).width(Stretch(1.0)); // Pushes right

                // The master control for the section
                SingleKnob::new(cx, Data::params, |p| &p.corrosion_amount, false, 120.0)
                    .class("large-center-knob");

                Element::new(cx).width(Stretch(1.0)); // Pushes left
            });

            // BOTTOM ROW: Noise Blend (Bottom-Left) and Stereo (Bottom-Right)
            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |p| &p.corrosion_noise_blend, false, 85.0);

                // Another spring in the middle pushing these to the bottom corners
                Element::new(cx).width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |p| &p.corrosion_stereo, false, 85.0);
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

fn build_nam_triangle(cx: &mut Context) {
    ZStack::new(cx, |cx| {
        Label::new(cx, "NAM")
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
            // TOP ROW: Model Selector (Centered)
            HStack::new(cx, |cx| {
                Element::new(cx).width(Stretch(1.0));
                // Assuming you have a dropdown/knob for the EnumParam
                SingleKnob::new(cx, Data::params, |p| &p.nam_model, false, 85.0);
                Element::new(cx).width(Stretch(1.0));
            });

            // BOTTOM ROW: Input and Output Gain
            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |p| &p.nam_input_gain, false, 85.0).on_change(
                    |cx, val| {
                        // Link inversely: 1.0 - val
                        let linked_val = 1.0 - val;
                        cx.emit(ParamEvent::<FloatParam>::BeginSetParameter(unsafe {
                            std::mem::transmute(&Data::params.get(cx).nam_output_gain)
                        }));
                        cx.emit(RawParamEvent::BeginSetParameter(
                            Data::params.get(cx).nam_output_gain.as_ptr(),
                        ));
                        cx.emit(ParamEvent::<FloatParam>::SetParameterNormalized(
                            unsafe { std::mem::transmute(&Data::params.get(cx).nam_output_gain) },
                            linked_val,
                        ));
                        cx.emit(RawParamEvent::SetParameterNormalized(
                            Data::params.get(cx).nam_output_gain.as_ptr(),
                            linked_val,
                        ));
                        cx.emit(ParamEvent::<FloatParam>::EndSetParameter(unsafe {
                            std::mem::transmute(&Data::params.get(cx).nam_output_gain)
                        }));
                        cx.emit(RawParamEvent::EndSetParameter(
                            Data::params.get(cx).nam_output_gain.as_ptr(),
                        ));
                    },
                );
                Element::new(cx).width(Stretch(1.0));
                SingleKnob::new(cx, Data::params, |p| &p.nam_output_gain, false, 85.0).on_change(
                    |cx, val| {
                        // Link inversely: 1.0 - val
                        let linked_val = 1.0 - val;
                        cx.emit(ParamEvent::<FloatParam>::BeginSetParameter(unsafe {
                            std::mem::transmute(&Data::params.get(cx).nam_input_gain)
                        }));
                        cx.emit(RawParamEvent::BeginSetParameter(
                            Data::params.get(cx).nam_input_gain.as_ptr(),
                        ));
                        cx.emit(ParamEvent::<FloatParam>::SetParameterNormalized(
                            unsafe { std::mem::transmute(&Data::params.get(cx).nam_input_gain) },
                            linked_val,
                        ));
                        cx.emit(RawParamEvent::SetParameterNormalized(
                            Data::params.get(cx).nam_input_gain.as_ptr(),
                            linked_val,
                        ));
                        cx.emit(ParamEvent::<FloatParam>::EndSetParameter(unsafe {
                            std::mem::transmute(&Data::params.get(cx).nam_input_gain)
                        }));
                        cx.emit(RawParamEvent::EndSetParameter(
                            Data::params.get(cx).nam_input_gain.as_ptr(),
                        ));
                    },
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

fn create_toggle_button<'a, L, F>(
    cx: &'a mut Context,
    label_text: &'static str,
    lens: L,
    params: &Arc<KickParams>,
    selector: F,
    class: &str,
    toggle_class: &str,
) -> Handle<'a, VStack>
where
    L: Lens<Target = bool> + Copy + 'static + Send + Sync,
    F: 'static + Clone + Fn(&KickParams) -> &BoolParam + Send + Sync,
{
    let params_arc = params.clone();
    let selector = selector.clone();

    VStack::new(cx, |cx| {
        Label::new(cx, label_text).hoverable(false);
    })
    .class(class)
    .toggle_class(toggle_class, lens)
    .focusable(true)
    .on_press(move |cx| {
        let param = selector(&params_arc);
        let current_value = param.value();
        let new_normalized_value = if current_value { 0.0 } else { 1.0 };

        let ptr = param.as_ptr();
        let param_static: &'static BoolParam = unsafe { std::mem::transmute(param) };

        cx.emit(ParamEvent::BeginSetParameter(param_static));
        cx.emit(RawParamEvent::BeginSetParameter(ptr));
        cx.emit(ParamEvent::SetParameterNormalized(
            param_static,
            new_normalized_value,
        ));
        cx.emit(RawParamEvent::SetParameterNormalized(
            ptr,
            new_normalized_value,
        ));
        cx.emit(ParamEvent::EndSetParameter(param_static));
        cx.emit(RawParamEvent::EndSetParameter(ptr));
    })
}

fn create_text_button<'a, L, F>(
    cx: &'a mut Context,
    label_text: &'static str,
    lens: L,
    params: &Arc<KickParams>,
    selector: F,
    class: &str,
    toggle_class: &str,
) -> Handle<'a, VStack>
where
    L: Lens<Target = bool> + Copy + 'static + Send + Sync,
    F: 'static + Clone + Fn(&KickParams) -> &BoolParam + Send + Sync,
{
    let params_arc = params.clone();
    let selector = selector.clone();

    let params_arc_down = params_arc.clone();
    let selector_down = selector.clone();

    let params_arc_up = params_arc.clone();
    let selector_up = selector.clone();

    VStack::new(cx, |cx| {
        Label::new(cx, label_text).hoverable(false);
    })
    .class(class)
    .toggle_class(toggle_class, lens)
    .focusable(true)
    .on_mouse_down(move |cx, _btn| {
        cx.focus();
        cx.set_active(true);

        params_arc_down.gui_trigger.fetch_add(1, Ordering::SeqCst);

        let param = selector_down(&params_arc_down);
        let ptr = param.as_ptr();
        let param_static: &'static BoolParam = unsafe { std::mem::transmute(param) };

        // Visual feedback only - set param to 1.0
        cx.emit(ParamEvent::BeginSetParameter(param_static));
        cx.emit(RawParamEvent::BeginSetParameter(ptr));
        cx.emit(ParamEvent::SetParameterNormalized(param_static, 1.0));
        cx.emit(RawParamEvent::SetParameterNormalized(ptr, 1.0));
        cx.emit(ParamEvent::EndSetParameter(param_static));
        cx.emit(RawParamEvent::EndSetParameter(ptr));
    })
    .on_double_click(move |cx, _btn| {
        // Capture the second half of a double-click as a trigger
        let params = params_arc.clone();
        params.gui_trigger.fetch_add(1, Ordering::SeqCst);

        let param = selector(&params);
        let ptr = param.as_ptr();
        let param_static: &'static BoolParam = unsafe { std::mem::transmute(param) };

        cx.emit(ParamEvent::SetParameterNormalized(param_static, 1.0));
        cx.emit(RawParamEvent::SetParameterNormalized(ptr, 1.0));
    })
    .on_mouse_up(move |cx, _btn| {
        cx.set_active(false);

        params_arc_up.gui_release.fetch_add(1, Ordering::SeqCst);

        let param = selector_up(&params_arc_up);
        let ptr = param.as_ptr();
        let param_static: &'static BoolParam = unsafe { std::mem::transmute(param) };

        // Visual feedback only - set param back to 0.0
        cx.emit(ParamEvent::BeginSetParameter(param_static));
        cx.emit(RawParamEvent::BeginSetParameter(ptr));
        cx.emit(ParamEvent::SetParameterNormalized(param_static, 0.0));
        cx.emit(RawParamEvent::SetParameterNormalized(ptr, 0.0));
        cx.emit(ParamEvent::EndSetParameter(param_static));
        cx.emit(RawParamEvent::EndSetParameter(ptr));
    })
}

fn build_preset_header(cx: &mut Context) {
    HStack::new(cx, |cx| {
        Label::new(cx, "Preset:").class("preset-label");

        PickList::new(
            cx,
            Data::factory_presets.map(|p| p.iter().map(|pr| pr.name.clone()).collect::<Vec<_>>()),
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
