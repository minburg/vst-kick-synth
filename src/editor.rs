use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::KickParams;

mod param_knob;
use self::param_knob::ParamKnob;

#[derive(Lens)]
struct Data {
    params: Arc<KickParams>,
}

impl Model for Data {}

pub(crate) fn default_state() -> Arc<ViziaState> {
    ViziaState::new(|| (400, 400))
}

pub(crate) fn create(
    params: Arc<KickParams>,
    editor_state: Arc<ViziaState>,
) -> Option<Box<dyn Editor>> {
    create_vizia_editor(editor_state, ViziaTheming::Custom, move |cx, _| {
        
        Data {
            params: params.clone(),
        }
        .build(cx);

        VStack::new(cx, |cx| {
            Label::new(cx, "KICK SYNTH")
                .font_size(30.0)
                .height(Pixels(50.0))
                .child_space(Stretch(1.0));

            HStack::new(cx, |cx| {
                VStack::new(cx, |cx| {
                    Label::new(cx, "Tune");
                    ParamKnob::new(cx, Data::params, |p| &p.tune, false);
                }).row_between(Pixels(10.0));

                VStack::new(cx, |cx| {
                    Label::new(cx, "Sweep");
                    ParamKnob::new(cx, Data::params, |p| &p.sweep, false);
                }).row_between(Pixels(10.0));
            })
            .col_between(Pixels(30.0))
            .child_space(Stretch(1.0));

            // Trigger Button
            let params_for_button = params.clone();
            Button::new(
                cx,
                move |cx| {
                    // Manual Trigger via Atomic
                    params_for_button.gui_trigger.store(true, Ordering::SeqCst);
                    
                    // Also update the param for visualization/automation if needed
                    let p_ref: &'static BoolParam = unsafe { std::mem::transmute(&params_for_button.trigger) };
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
                |cx| Label::new(cx, "TRIGGER")
            )
            .width(Pixels(150.0))
            .height(Pixels(60.0))
            .background_color(Color::rgb(40, 40, 40))
            .border_color(Color::rgb(80, 80, 80))
            .border_width(Pixels(1.0));

        })
        .child_space(Stretch(1.0))
        .row_between(Pixels(30.0));
    })
}
