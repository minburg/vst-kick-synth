use crate::editor::single_knob::SingleKnob;
use nih_plug::prelude::*;
use nih_plug_vizia::assets::register_noto_sans_light;
use nih_plug_vizia::vizia::image::load_from_memory;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::{ParamEvent, RawParamEvent, ResizeHandle};
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::KickParams;

mod param_knob;
mod single_knob;

pub const ORBITRON_TTF: &[u8] = include_bytes!("resource/fonts/Orbitron-Regular.ttf");
pub const COMFORTAA_LIGHT_TTF: &[u8] = include_bytes!("resource/fonts/Comfortaa-Light.ttf");
pub const COMFORTAA: &str = "Comfortaa";

const BG_IMAGE_BYTES: &[u8] = include_bytes!("resource/images/background_image.png");

use self::param_knob::ParamKnob;

#[derive(Lens)]
struct Data {
    params: Arc<KickParams>,
}

impl Model for Data {}

pub(crate) fn default_state() -> Arc<ViziaState> {
    ViziaState::new(|| (1000, 500))
}

pub(crate) fn create(
    params: Arc<KickParams>,
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

        if let Err(e) = cx.add_stylesheet(include_style!("/src/resource/style.css")) {
            nih_log!("CSS Error: {:?}", e);
        }

        Data {
            params: params.clone(),
        }
        .build(cx);

        VStack::new(cx, |cx| {
            VStack::new(cx, |cx| {
                Label::new(cx, "CONVOLUTION'S KICK SYNTH").class("header-title");
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

                SingleKnob::new(cx, Data::params, |params| &params.drive, false)
                    .width(Stretch(1.0));
            })
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
            .class("finetune-section-inner");

            HStack::new(cx, |cx| {
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
                .width(Stretch(0.45))
                .height(Stretch(0.5))
                .child_left(Stretch(1.0))
                .child_right(Stretch(1.0))
                .child_top(Stretch(0.08))
                .child_bottom(Stretch(0.08));
            })
            .width(Pixels(250.0));
        })
        .width(Stretch(1.0))
        .height(Stretch(1.0))
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

    DebugWrapper::new(cx, label_text, move |cx| {
        Label::new(cx, label_text).hoverable(false);
    })
    .class(class)
    .toggle_class(toggle_class, lens)
    .focusable(true)
    .on_mouse_down(move |cx, _btn| {
        cx.focus();
        cx.set_active(true);

        params_arc.gui_trigger.store(true, Ordering::SeqCst);

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
