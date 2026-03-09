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

        VStack::new(cx, |cx| {
            VStack::new(cx, |cx| {
                Label::new(cx, "CONVOLUTION'S Kick Synth").class("header-title");
                HStack::new(cx, |cx| {
                    Label::new(cx, "Check for Updates")
                        .class("update-link")
                        .on_press(|_| {
                            if let Err(e) = webbrowser::open("https://github.com/minburg/vst-kick-synth/releases") {
                                nih_log!("Failed to open browser: {}", e);
                            }
                        });
                    Label::new(cx, "v0.1.0").class("header-version-title");
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
                    .width(Stretch(1.0))
                    .child_space(Stretch(1.0))
                    .child_top(Stretch(0.01))
                    .child_bottom(Stretch(0.01))
                    .class("link-section");
            })
                .row_between(Pixels(10.0))
                .child_space(Stretch(1.0))
                .class("title-section");

            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |params| &params.tune, false).width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.sweep, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.pitch_decay, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.analog_variation, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.drive, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.drive_model, false)
                    .width(Stretch(1.0));
            })
            .width(Stretch(1.0))
            .left(Stretch(0.05))
            .right(Stretch(0.05))
            .class("finetune-section-inner");

            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |params| &params.tex_amt, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.tex_decay, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.tex_variation, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.tex_type, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.tex_tone, false)
                    .width(Stretch(1.0));
            })
            .width(Stretch(1.0))
            .left(Stretch(0.05))
            .right(Stretch(0.05))
            .class("finetune-section-inner");

            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |params| &params.corrosion_frequency, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.corrosion_width, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.corrosion_noise_blend, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.corrosion_stereo, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.corrosion_amount, false)
                    .width(Stretch(1.0));
            })
                .width(Stretch(1.0))
                .left(Stretch(0.05))
                .right(Stretch(0.05))
                .class("finetune-section-inner");

            HStack::new(cx, |cx| {
                SingleKnob::new(cx, Data::params, |params| &params.attack, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.decay, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.sustain, false)
                    .width(Stretch(1.0));

                SingleKnob::new(cx, Data::params, |params| &params.release, false)
                    .width(Stretch(1.0));
            })
            .width(Stretch(1.0))
            .left(Stretch(0.05))
            .right(Stretch(0.05))
            .class("finetune-section-inner");

            HStack::new(cx, |cx| {
                MyPeakMeter::new(
                    cx,
                    Data::peak_meter_l.map(|peak_meter_l| {
                        util::gain_to_db(peak_meter_l.load(Ordering::Relaxed))
                    }),
                    Some(Duration::from_millis(30)),
                    true
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
                    .child_space(Stretch(1.0)); // Center text horizontally and vertically


                MyPeakMeter::new(
                    cx,
                    Data::peak_meter_r.map(|peak_meter_r| {
                        util::gain_to_db(peak_meter_r.load(Ordering::Relaxed))
                    }),
                    Some(Duration::from_millis(30)),
                    false
                )
                    .class("vu-meter-no-text")
                    .top(Stretch(0.05))
                    .bottom(Stretch(0.05))
                    .width(Stretch(1.0))
                    .height(Pixels(40.0));
            })
                .child_space(Stretch(1.0))
                .width(Stretch(1.0))
                .height(Stretch(0.7));
        })
        .class("main-gui");

        ResizeHandle::new(cx);
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
) -> Handle<'a, DebugWrapper>
where
    L: Lens<Target = bool> + Copy + 'static + Send + Sync,
    F: 'static + Clone + Fn(&KickParams) -> &BoolParam + Send + Sync,
{
    let params_arc = params.clone();
    let selector = selector.clone();

    let params_arc_down = params_arc.clone();
    let selector_down = selector.clone();

    DebugWrapper::new(cx, label_text, move |cx| {
        Label::new(cx, label_text).hoverable(false);
    })
    .class(class)
    .toggle_class(toggle_class, lens)
    .focusable(true)
    .on_mouse_down(move |cx, _btn| {
        cx.focus();
        cx.set_active(true);

        params_arc_down.gui_trigger.store(true, Ordering::SeqCst);

        let param = selector_down(&params_arc_down);
        let ptr = param.as_ptr();
        let param_static: &'static BoolParam = unsafe { std::mem::transmute(param) };

        // --- PHASE 1: OPEN THE GESTURE & PERFORM (set to 1.0) ---
        cx.emit(ParamEvent::BeginSetParameter(param_static));
        cx.emit(RawParamEvent::BeginSetParameter(ptr));
        cx.emit(ParamEvent::SetParameterNormalized(param_static, 1.0));
        cx.emit(RawParamEvent::SetParameterNormalized(ptr, 1.0));

        // --- PHASE 2: START THE TIMER TO CLOSE THE GESTURE for the initial press ---
        let gesture_duration = std::time::Duration::from_millis(20);
        cx.add_timer(
            gesture_duration,
            Some(gesture_duration),
            move |cx, action| {
                if let TimerAction::Stop = action {
                    cx.emit(ParamEvent::EndSetParameter(param_static));
                    cx.emit(RawParamEvent::EndSetParameter(ptr));
                }
            },
        );

        // --- Reset param back to 0.0 after a delay ---
        let reset_delay = std::time::Duration::from_millis(50);
        cx.add_timer(reset_delay, None, move |cx, _| {
            cx.emit(ParamEvent::BeginSetParameter(param_static));
            cx.emit(RawParamEvent::BeginSetParameter(ptr));
            cx.emit(ParamEvent::SetParameterNormalized(param_static, 0.0));
            cx.emit(RawParamEvent::SetParameterNormalized(ptr, 0.0));
            cx.emit(ParamEvent::EndSetParameter(param_static));
            cx.emit(RawParamEvent::EndSetParameter(ptr));
        });
    })
    .on_mouse_up(move |cx, _btn| {
        cx.focus();
        cx.set_active(true);

        params_arc.gui_release.store(true, Ordering::SeqCst);

        let param = selector(&params_arc);
        let ptr = param.as_ptr();
        let param_static: &'static BoolParam = unsafe { std::mem::transmute(param) };

        // --- PHASE 1: OPEN THE GESTURE & PERFORM (set to 1.0) ---
        cx.emit(ParamEvent::BeginSetParameter(param_static));
        cx.emit(RawParamEvent::BeginSetParameter(ptr));
        cx.emit(ParamEvent::SetParameterNormalized(param_static, 1.0));
        cx.emit(RawParamEvent::SetParameterNormalized(ptr, 1.0));

        // --- PHASE 2: START THE TIMER TO CLOSE THE GESTURE for the initial press ---
        let gesture_duration = std::time::Duration::from_millis(20);
        cx.add_timer(
            gesture_duration,
            Some(gesture_duration),
            move |cx, action| {
                if let TimerAction::Stop = action {
                    cx.emit(ParamEvent::EndSetParameter(param_static));
                    cx.emit(RawParamEvent::EndSetParameter(ptr));
                }
            },
        );

        // --- Reset param back to 0.0 after a delay ---
        let reset_delay = std::time::Duration::from_millis(50);
        cx.add_timer(reset_delay, None, move |cx, _| {
            cx.emit(ParamEvent::BeginSetParameter(param_static));
            cx.emit(RawParamEvent::BeginSetParameter(ptr));
            cx.emit(ParamEvent::SetParameterNormalized(param_static, 0.0));
            cx.emit(RawParamEvent::SetParameterNormalized(ptr, 0.0));
            cx.emit(ParamEvent::EndSetParameter(param_static));
            cx.emit(RawParamEvent::EndSetParameter(ptr));
        });
    })
}

pub struct DebugWrapper {
    name: String,
}

impl DebugWrapper {
    // FIX: Added lifetime 'a to tie the Handle to the Context
    pub fn new<'a, F>(cx: &'a mut Context, name: &str, content: F) -> Handle<'a, Self>
    where
        F: FnOnce(&mut Context),
    {
        Self {
            name: name.to_string(),
        }
        .build(cx, |cx| {
            (content)(cx);
        })
    }
}

impl View for DebugWrapper {
    fn element(&self) -> Option<&'static str> {
        Some("debug-wrapper")
    }

    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|window_event, _| match window_event {
            WindowEvent::MouseEnter => {
                nih_log!("[{}] Mouse ENTER. Bounds: {:?}", self.name, cx.bounds());
            }
            WindowEvent::MouseLeave => {
                nih_log!("[{}] Mouse LEAVE", self.name);
            }
            WindowEvent::MouseDown(btn) => {
                let mouse = cx.mouse();
                nih_log!(
                    "[{}] Mouse DOWN ({:?}). \n\t-> Mouse Pos: ({}, {})\n\t-> Btn Bounds: ({}, {}, {}, {})",
                    self.name,
                    btn,
                    mouse.cursorx,
                    mouse.cursory,
                    cx.bounds().x,
                    cx.bounds().y,
                    cx.bounds().w,
                    cx.bounds().h
                );
            }
            WindowEvent::MouseUp(btn) => {
                nih_log!("[{}] Mouse UP ({:?})", self.name, btn);
            }
            WindowEvent::FocusIn => {
                nih_log!("[{}] Focus GAINED", self.name);
            }
            WindowEvent::FocusOut => {
                nih_log!("[{}] Focus LOST", self.name);
            }
            _ => {}
        });
    }
}
