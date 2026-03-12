use std::sync::Arc;
use nih_plug::prelude::Param;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::param_base::ParamWidgetBase;

#[derive(Debug)]
pub enum SingleKnobEvent {
    BeginSetParam,
    SetParam(f32),
    EndSetParam,
}

#[derive(Lens)]
pub struct SingleKnob {
    param_base: ParamWidgetBase,
    size: f32,
    on_change: Option<Arc<dyn Fn(&mut EventContext, f32) + Send + Sync>>,
}

impl SingleKnob {
    pub fn new<L, Params, P, FMap>(
        cx: &mut Context,
        params: L,
        params_to_param: FMap,
        _centered: bool,
        size: f32,
    ) -> Handle<'_, Self>
    where
        L: Lens<Target = Params> + Clone + Copy,
        Params: 'static,
        P: Param + 'static,
        FMap: Fn(&Params) -> &P + Copy + 'static,
    {
        Self {
            param_base: ParamWidgetBase::new(cx, params.clone(), params_to_param),
            size,
            on_change: None,
        }
        .build(
            cx,
            ParamWidgetBase::build_view(params, params_to_param, move |cx, param_data| {
                VStack::new(cx, |cx| {
                    // TOP LABEL
                    Label::new(
                        cx,
                        params.map(move |params| params_to_param(params).name().to_owned()),
                    )
                    // Removed .space(Stretch(1.0)) so it doesn't float away
                    .class("single-knob-label");

                    // THE KNOB
                    Knob::custom(
                        cx,
                        param_data.param().default_normalized_value(),
                        params.map(move |params| {
                            params_to_param(params).unmodulated_normalized_value()
                        }),
                        move |cx, lens| {
                            ZStack::new(cx, |cx| {
                                // Transparent "Hit Surface"
                                Element::new(cx)
                                    .width(Stretch(1.0)) // Fills the parent Knob
                                    .height(Stretch(1.0))
                                    .border_radius(Pixels(size / 2.0))
                                    .class("single-knob-hitbox");

                                // Vintage Knob Image
                                Element::new(cx)
                                    .class("vintage-knob")
                                    .width(Stretch(1.0)) // Fills the parent Knob
                                    .height(Stretch(1.0))
                                    .border_radius(Pixels(size / 2.0))
                                    .rotate(lens.map(|val| Angle::Deg(val * 300.0 - 18.0)));
                            })
                        },
                    )
                    .border_radius(Pixels(size / 2.0))
                    // Move the explicit pixel sizing to the Knob widget itself!
                    .width(Pixels(size))
                    .height(Pixels(size))
                    // Removed .space(Stretch(5.0))
                    .on_mouse_down(move |cx, _button| {
                        cx.emit(SingleKnobEvent::BeginSetParam);
                    })
                    .on_changing(move |cx, val| {
                        cx.emit(SingleKnobEvent::SetParam(val));
                    })
                    .on_mouse_up(move |cx, _button| {
                        cx.emit(SingleKnobEvent::EndSetParam);
                    });

                    // BOTTOM LABEL
                    Label::new(
                        cx,
                        params.map(move |params| {
                            params_to_param(params)
                                .normalized_value_to_string(
                                    params_to_param(params)
                                        .modulated_normalized_value()
                                        .to_owned(),
                                    true,
                                )
                                .to_owned()
                        }),
                    )
                    // Removed .space(Stretch(1.0))
                    .class("single-knob-label");
                })
                .class("single-knob-container")
                // child_space(Stretch(1.0)) acts as a master centering spring for the tightly packed group
                .child_space(Stretch(1.0))
                // Adds a fixed, predictable pixel gap between the top label, the knob, and the bottom label
                .row_between(Pixels(4.0));
            }),
        )
    }
}

impl View for SingleKnob {
    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|param_change_event, _| match param_change_event {
            SingleKnobEvent::BeginSetParam => {
                self.param_base.begin_set_parameter(cx);
            }
            SingleKnobEvent::SetParam(val) => {
                self.param_base.set_normalized_value(cx, *val);
                if let Some(on_change) = &self.on_change {
                    (on_change)(cx, *val);
                }
            }
            SingleKnobEvent::EndSetParam => {
                self.param_base.end_set_parameter(cx);
            }
        });
    }
}

pub trait SingleKnobExt {
    fn on_change<F: Fn(&mut EventContext, f32) + Send + Sync + 'static>(self, callback: F) -> Self;
}

impl SingleKnobExt for Handle<'_, SingleKnob> {
    fn on_change<F: Fn(&mut EventContext, f32) + Send + Sync + 'static>(self, callback: F) -> Self {
        self.modify(|single_knob| single_knob.on_change = Some(Arc::new(callback)))
    }
}
