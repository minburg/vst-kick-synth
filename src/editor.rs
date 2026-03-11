use crate::editor::single_knob::SingleKnob;
use nih_plug::prelude::*;
use nih_plug_vizia::assets::register_noto_sans_light;
use nih_plug_vizia::vizia::image::load_from_memory;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::{ParamEvent, RawParamEvent, ResizeHandle};
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::editor::my_peak_meter::MyPeakMeter;
use crate::KickParams;

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

use self::param_knob::ParamKnob;

#[derive(Lens)]
struct Data {
    params: Arc<KickParams>,
    peak_meter_l: Arc<AtomicF32>,
    peak_meter_r: Arc<AtomicF32>,
}

impl Model for Data {}

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
        }
        .build(cx);

        // --------------------------------------------------------------------------------------------------------------- UI

        HStack::new(cx, |cx| {
            // ZONE 1: THE SOURCE (Generators)
            VStack::new(cx, |cx| {
                build_pitch_core_diamond(cx);
                build_texture_satellite(cx);
            })
            .width(Stretch(1.0))
            .class("zone-source");

            // ZONE 2: THE BODY (Amp Envelope)
            VStack::new(cx, |cx| {
                build_amp_envelope_sliders(cx);
            })
            .width(Stretch(1.0))
            .class("zone-body");

            // ZONE 3: THE MANGLE (Destruction)
            VStack::new(cx, |cx| {
                build_drive_pill(cx);
                build_corrosion_pentagon(cx);
                build_nam_triangle(cx);
            })
            .width(Stretch(1.0))
            .class("zone-mangle");
        })
        .width(Stretch(1.0))
        .height(Stretch(1.0));
        // .class("main-gui");

        // VStack::new(cx, |cx| {
        //     VStack::new(cx, |cx| {
        //         Label::new(cx, "CONVOLUTION'S Kick Synth").class("header-title");
        //         HStack::new(cx, |cx| {
        //             Label::new(cx, "Check for Updates")
        //                 .class("update-link")
        //                 .on_press(|_| {
        //                     if let Err(e) = webbrowser::open("https://github.com/minburg/vst-kick-synth/releases") {
        //                         nih_log!("Failed to open browser: {}", e);
        //                     }
        //                 });
        //             Label::new(cx, "v0.1.0").class("header-version-title");
        //             Element::new(cx)
        //                 .class("insta-button")
        //                 .on_press(|_| {
        //                     let _ = webbrowser::open("https://www.instagram.com/convolution.official/");
        //                 });
        //             Element::new(cx)
        //                 .class("spotify-button").opacity(0.5)
        //                 .on_press(|_| {
        //                     let _ = webbrowser::open("https://open.spotify.com/artist/7k0eMwQbplT3Zyyy0DalRL?si=aalp-7GQQ2O_cZRodAlsNg");
        //                 });
        //         })
        //             .width(Stretch(1.0))
        //             .child_space(Stretch(1.0))
        //             .child_top(Stretch(0.01))
        //             .child_bottom(Stretch(0.01))
        //             .class("link-section");
        //     })
        //         .row_between(Pixels(10.0))
        //         .child_space(Stretch(1.0))
        //         .class("title-section");
        //
        //     HStack::new(cx, |cx| {
        //         SingleKnob::new(cx, Data::params, |params| &params.tune, false).width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.sweep, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.pitch_decay, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.analog_variation, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.drive, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.drive_model, false)
        //             .width(Stretch(1.0));
        //     })
        //     .width(Stretch(1.0))
        //     .left(Stretch(0.05))
        //     .right(Stretch(0.05))
        //     .class("finetune-section-inner");
        //
        //     HStack::new(cx, |cx| {
        //         SingleKnob::new(cx, Data::params, |params| &params.tex_amt, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.tex_decay, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.tex_variation, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.tex_type, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.tex_tone, false)
        //             .width(Stretch(1.0));
        //     })
        //     .width(Stretch(1.0))
        //     .left(Stretch(0.05))
        //     .right(Stretch(0.05))
        //     .class("finetune-section-inner");
        //
        //     HStack::new(cx, |cx| {
        //         SingleKnob::new(cx, Data::params, |params| &params.corrosion_frequency, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.corrosion_width, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.corrosion_noise_blend, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.corrosion_stereo, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.corrosion_amount, false)
        //             .width(Stretch(1.0));
        //     })
        //         .width(Stretch(1.0))
        //         .left(Stretch(0.05))
        //         .right(Stretch(0.05))
        //         .class("finetune-section-inner");
        //
        //     HStack::new(cx, |cx| {
        //         SingleKnob::new(cx, Data::params, |params| &params.attack, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.decay, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.sustain, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.release, false)
        //             .width(Stretch(1.0));
        //     })
        //     .width(Stretch(1.0))
        //     .left(Stretch(0.05))
        //     .right(Stretch(0.05))
        //     .class("finetune-section-inner");
        //
        //     HStack::new(cx, |cx| {
        //         SingleKnob::new(cx, Data::params, |params| &params.nam_model, false)
        //             .width(Stretch(1.0));
        //
        //         VStack::new(cx, |cx| {
        //             Label::new(cx, Data::params.map(|p| p.nam_status_text.read().clone()))
        //                 .class("nam-status-label")
        //                 .toggle_class("success", Data::params.map(|p| p.nam_is_loaded.load(Ordering::Relaxed)))
        //                 .toggle_class("error", Data::params.map(|p| !p.nam_is_loaded.load(Ordering::Relaxed)));
        //         })
        //         .width(Stretch(1.0))
        //         .child_space(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.nam_input_gain, false)
        //             .width(Stretch(1.0));
        //
        //         SingleKnob::new(cx, Data::params, |params| &params.nam_output_gain, false)
        //             .width(Stretch(1.0));
        //     })
        //     .width(Stretch(1.0))
        //     .left(Stretch(0.05))
        //     .right(Stretch(0.05))
        //     .class("finetune-section-inner");
        //
        //     HStack::new(cx, |cx| {
        //         MyPeakMeter::new(
        //             cx,
        //             Data::peak_meter_l.map(|peak_meter_l| {
        //                 util::gain_to_db(peak_meter_l.load(Ordering::Relaxed))
        //             }),
        //             Some(Duration::from_millis(30)),
        //             true
        //         )
        //             .class("vu-meter-no-text")
        //             .top(Stretch(0.05))
        //             .bottom(Stretch(0.05))
        //             .width(Stretch(1.0))
        //             .height(Pixels(40.0));
        //
        //         // Trigger Button
        //         create_text_button(
        //             cx,
        //             "TRIGGER",
        //             Data::params.map(|p| p.trigger.value()),
        //             &params,
        //             |p| &p.trigger,
        //             "distortion-param-button",
        //             "active",
        //         )
        //             .left(Stretch(0.1))
        //             .right(Stretch(0.1))
        //             .top(Stretch(0.05))
        //             .bottom(Stretch(0.05))
        //             .width(Stretch(1.0))
        //             .height(Pixels(60.0))
        //             .child_space(Stretch(1.0)); // Center text horizontally and vertically
        //
        //         MyPeakMeter::new(
        //             cx,
        //             Data::peak_meter_r.map(|peak_meter_r| {
        //                 util::gain_to_db(peak_meter_r.load(Ordering::Relaxed))
        //             }),
        //             Some(Duration::from_millis(30)),
        //             false
        //         )
        //             .class("vu-meter-no-text")
        //             .top(Stretch(0.05))
        //             .bottom(Stretch(0.05))
        //             .width(Stretch(1.0))
        //             .height(Pixels(40.0));
        //     })
        //         .child_space(Stretch(1.0))
        //         .width(Stretch(1.0))
        //         .height(Stretch(0.7));
        // })
        // .class("main-gui");

        // --------------------------------------------------------------------------------------------------------------- UI

        ResizeHandle::new(cx);
    })
}

fn build_pitch_core_diamond(cx: &mut Context) {
    // Inside your UI build function or a helper:
    VStack::new(cx, |cx| {
        // TOP ROW: Tune (Centered)
        HStack::new(cx, |cx| {
            Element::new(cx).width(Stretch(1.0)); // Invisible spring pushes right
            SingleKnob::new(cx, Data::params, |p| &p.tune, false, 95.0);
            Element::new(cx).width(Stretch(1.0)); // Invisible spring pushes left
        });

        // MIDDLE ROW: Drift & Decay (Pushed to edges)
        HStack::new(cx, |cx| {
            SingleKnob::new(cx, Data::params, |p| &p.analog_variation, false, 95.0);
            Element::new(cx).width(Stretch(1.0)); // Spring in the middle pushes them apart
            SingleKnob::new(cx, Data::params, |p| &p.pitch_decay, false, 95.0);
        });

        // BOTTOM ROW: Sweep (Centered)
        HStack::new(cx, |cx| {
            Element::new(cx).width(Stretch(1.0));
            SingleKnob::new(cx, Data::params, |p| &p.sweep, false, 95.0);
            Element::new(cx).width(Stretch(1.0));
        });
    })
    .child_space(Stretch(1.0)); // Centers the whole block vertically if needed
}

fn build_texture_satellite(cx: &mut Context) {
    VStack::new(cx, |cx| {
        // TOP ROW: Texture Type (12 o'clock)
        HStack::new(cx, |cx| {
            Element::new(cx).width(Stretch(1.0)); // Pushes right
            SingleKnob::new(cx, Data::params, |p| &p.tex_type, false, 95.0);
            Element::new(cx).width(Stretch(1.0)); // Pushes left
        });

        // MIDDLE ROW: Variation (9 o'clock), Amount (Center), Tone (3 o'clock)
        HStack::new(cx, |cx| {
            SingleKnob::new(cx, Data::params, |p| &p.tex_variation, false, 95.0);

            Element::new(cx).width(Stretch(1.0)); // Space between Variation and Center

            // This is the core parameter, maybe give it a CSS class to make it bigger!
            SingleKnob::new(cx, Data::params, |p| &p.tex_amt, false, 150.0).class("large-center-knob");

            Element::new(cx).width(Stretch(1.0)); // Space between Center and Tone

            SingleKnob::new(cx, Data::params, |p| &p.tex_tone, false, 95.0);
        });

        // BOTTOM ROW: Texture Decay (6 o'clock)
        HStack::new(cx, |cx| {
            Element::new(cx).width(Stretch(1.0));
            SingleKnob::new(cx, Data::params, |p| &p.tex_decay, false, 95.0);
            Element::new(cx).width(Stretch(1.0));
        });
    })
    .child_space(Stretch(1.0)); // Centers the entire satellite formation
}

fn build_amp_envelope_sliders(cx: &mut Context) {
    HStack::new(cx, |cx| {
        VerticalParamSlider::new(cx, Data::params, |p| &p.attack).width(Stretch(0.5));

        VerticalParamSlider::new(cx, Data::params, |p| &p.decay).width(Stretch(0.5));

        VerticalParamSlider::new(cx, Data::params, |p| &p.sustain).width(Stretch(0.5));

        VerticalParamSlider::new(cx, Data::params, |p| &p.release).width(Stretch(0.5));
    })
    .child_space(Stretch(1.0))
    .child_top(Stretch(0.8))
    .child_bottom(Stretch(0.8))
    .col_between(Pixels(20.0))
    .height(Stretch(0.4)) 
    .width(Stretch(0.3));
}

fn build_drive_pill(cx: &mut Context) {
    HStack::new(cx, |cx| {
        SingleKnob::new(cx, Data::params, |p| &p.drive_model, false, 95.0);

        SingleKnob::new(cx, Data::params, |p| &p.drive, false, 95.0);
    });
}

fn build_corrosion_pentagon(cx: &mut Context) {
    VStack::new(cx, |cx| {
        // TOP ROW: Frequency (Top-Left) and Width (Top-Right)
        HStack::new(cx, |cx| {
            SingleKnob::new(cx, Data::params, |p| &p.corrosion_frequency, false, 95.0);

            // This spring sits between the knobs, pushing them to the far left and right edges
            Element::new(cx).width(Stretch(1.0));

            SingleKnob::new(cx, Data::params, |p| &p.corrosion_width, false, 95.0);
        });

        // MIDDLE ROW: The main Corrosion Amount (Center)
        HStack::new(cx, |cx| {
            Element::new(cx).width(Stretch(1.0)); // Pushes right

            // The master control for the section
            SingleKnob::new(cx, Data::params, |p| &p.corrosion_amount, false, 150.0)
                .class("large-center-knob");

            Element::new(cx).width(Stretch(1.0)); // Pushes left
        });

        // BOTTOM ROW: Noise Blend (Bottom-Left) and Stereo (Bottom-Right)
        HStack::new(cx, |cx| {
            SingleKnob::new(cx, Data::params, |p| &p.corrosion_noise_blend, false, 95.0);

            // Another spring in the middle pushing these to the bottom corners
            Element::new(cx).width(Stretch(1.0));

            SingleKnob::new(cx, Data::params, |p| &p.corrosion_stereo, false, 95.0);
        });
    })
    .child_space(Stretch(1.0)); // Adds padding so it doesn't touch the exact edges of the container
}

fn build_nam_triangle(cx: &mut Context) {
    VStack::new(cx, |cx| {
        // TOP ROW: Model Selector (Centered)
        HStack::new(cx, |cx| {
            Element::new(cx).width(Stretch(1.0));
            // Assuming you have a dropdown/knob for the EnumParam
            SingleKnob::new(cx, Data::params, |p| &p.nam_model, false, 95.0);
            Element::new(cx).width(Stretch(1.0));
        });

        // BOTTOM ROW: Input and Output Gain
        HStack::new(cx, |cx| {
            SingleKnob::new(cx, Data::params, |p| &p.nam_input_gain, false, 95.0);
            Element::new(cx).width(Stretch(1.0)); // Pushes them slightly apart
            SingleKnob::new(cx, Data::params, |p| &p.nam_output_gain, false, 95.0);
        });
    });
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
