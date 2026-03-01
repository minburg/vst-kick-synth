use crate::editor::single_knob::SingleKnob;
use nih_plug::prelude::*;
use nih_plug_vizia::assets::register_noto_sans_light;
use nih_plug_vizia::vizia::image::load_from_memory;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::ResizeHandle;
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
                Label::new(cx, "CONVOLUTION'S KICK SYNTH")
                    // .font_size(30.0)
                    // .height(Pixels(50.0))
                    // .left(Stretch(1.0))
                    // .right(Stretch(1.0))
                    .class("header-title");
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

            // Trigger Button
            let params_for_button = params.clone();
            Button::new(
                cx,
                move |cx| {
                    // Manual Trigger via Atomic
                    params_for_button.gui_trigger.store(true, Ordering::SeqCst);

                    // Also update the param for visualization/automation if needed
                    let p_ref: &'static BoolParam =
                        unsafe { std::mem::transmute(&params_for_button.trigger) };
                    use nih_plug_vizia::widgets::ParamEvent;
                    cx.emit(ParamEvent::BeginSetParameter(p_ref));
                    cx.emit(ParamEvent::SetParameterNormalized(p_ref, 1.0));
                    cx.emit(ParamEvent::EndSetParameter(p_ref));

                    // Reset param after 50ms
                    cx.add_timer(std::time::Duration::from_millis(50), None, move |cx, _| {
                        cx.emit(ParamEvent::BeginSetParameter(p_ref));
                        cx.emit(ParamEvent::SetParameterNormalized(p_ref, 0.0));
                        cx.emit(ParamEvent::EndSetParameter(p_ref));
                    });
                },
                |cx| Label::new(cx, "TRIGGER"),
            )
            .class("distortion-param-button")
            .left(Stretch(1.0))
            .right(Stretch(1.0))
            .width(Pixels(150.0))
            .height(Pixels(60.0));
        })
        .width(Stretch(1.0))
        .height(Stretch(1.0))
        .class("main-gui");
        ResizeHandle::new(cx);
    })
}
