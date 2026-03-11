//! A vertical slider that integrates with NIH-plug's [`Param`] types.

use nih_plug::prelude::Param;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::param_base::ParamWidgetBase;

use super::util::{self, ModifiersExt};

/// When shift+dragging a parameter, one pixel dragged corresponds to this much change in the
/// normalized parameter.
const GRANULAR_DRAG_MULTIPLIER: f32 = 0.1;

/// A vertical slider that integrates with NIH-plug's [`Param`] types. Use the
/// [`set_style()`][VerticalParamSliderExt::set_style()] method to change how the value gets displayed.
#[derive(Lens)]
pub struct VerticalParamSlider {
    param_base: ParamWidgetBase,

    /// Will be set to `true` when the field gets Alt+Click'ed which will replace the label with a
    /// text box.
    text_input_active: bool,
    /// Will be set to `true` if we're dragging the parameter.
    drag_active: bool,
    /// We keep track of the start coordinate and normalized value when holding down Shift while
    /// dragging for higher precision dragging.
    granular_drag_status: Option<GranularDragStatus>,

    // These fields are set through modifiers:
    use_scroll_wheel: bool,
    scrolled_lines: f32,
    style: VerticalParamSliderStyle,
    label_override: Option<String>,
}

/// How the [`VerticalParamSlider`] should display its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Data)]
pub enum VerticalParamSliderStyle {
    Centered,
    /// Always fill the bar starting from the bottom.
    FromBottom,
    FromMidPoint,
    CurrentStep { even: bool },
    CurrentStepLabeled { even: bool },
}

enum VerticalParamSliderEvent {
    CancelTextInput,
    TextInput(String),
}

#[derive(Debug, Clone, Copy)]
pub struct GranularDragStatus {
    /// The mouse's Y-coordinate when the granular drag was started.
    pub starting_y_coordinate: f32,
    pub starting_value: f32,
}

impl VerticalParamSlider {
    pub fn new<L, Params, P, FMap>(
        cx: &mut Context,
        params: L,
        params_to_param: FMap,
    ) -> Handle<'_, Self>
    where
        L: Lens<Target = Params> + Clone,
        Params: 'static,
        P: Param + 'static,
        FMap: Fn(&Params) -> &P + Copy + 'static,
    {
        Self {
            param_base: ParamWidgetBase::new(cx, params, params_to_param),

            text_input_active: false,
            drag_active: false,
            granular_drag_status: None,

            use_scroll_wheel: true,
            scrolled_lines: 0.0,
            style: VerticalParamSliderStyle::Centered,
            label_override: None,
        }
        .build(cx, |cx| {
            ParamWidgetBase::build_view(params, params_to_param, move |cx, param_data| {
                    Binding::new(cx, VerticalParamSlider::style, move |cx, style| {
                        let style = style.get(cx);

                        let unmodulated_normalized_value_lens =
                            param_data.make_lens(|param| param.unmodulated_normalized_value());
                        let display_value_lens = param_data.make_lens(|param| {
                            param.normalized_value_to_string(param.unmodulated_normalized_value(), true)
                        });

                        let fill_start_delta_lens =
                            unmodulated_normalized_value_lens.map(move |current_value| {
                                Self::compute_fill_start_delta(
                                    style,
                                    param_data.param(),
                                    *current_value,
                                )
                            });

                        let modulation_start_delta_lens = param_data.make_lens(move |param| {
                            Self::compute_modulation_fill_start_delta(style, param)
                        });

                        let make_preview_value_lens = move |normalized_value| {
                            param_data.make_lens(move |param| {
                                param.normalized_value_to_string(normalized_value, true)
                            })
                        };

                        Binding::new(
                            cx,
                            VerticalParamSlider::text_input_active,
                            move |cx, text_input_active| {
                                if text_input_active.get(cx) {
                                    Self::text_input_view(cx, display_value_lens);
                                } else {
                                    ZStack::new(cx, |cx| {
                                        Self::slider_fill_view(
                                            cx,
                                            fill_start_delta_lens,
                                            modulation_start_delta_lens,
                                        );
                                        Self::slider_label_view(
                                            cx,
                                            param_data.param(),
                                            style,
                                            display_value_lens,
                                            make_preview_value_lens,
                                            VerticalParamSlider::label_override,
                                        );
                                    })
                                        .width(Stretch(1.0))
                                        .height(Stretch(1.0))
                                        .hoverable(false);
                                }
                            },
                        );
                    });
                })(cx);
            })
            .class("vertical-param-slider")
    }

    fn text_input_view(cx: &mut Context, display_value_lens: impl Lens<Target = String>) {
        Textbox::new(cx, display_value_lens)
            .class("value-entry")
            .on_submit(|cx, string, success| {
                if success {
                    cx.emit(VerticalParamSliderEvent::TextInput(string))
                } else {
                    cx.emit(VerticalParamSliderEvent::CancelTextInput);
                }
            })
            .on_cancel(|cx| {
                cx.emit(VerticalParamSliderEvent::CancelTextInput);
            })
            .on_build(|cx| {
                cx.emit(TextEvent::StartEdit);
                cx.emit(TextEvent::SelectAll);
            })
            .class("align_center")
            .child_top(Stretch(1.0))
            .child_bottom(Stretch(1.0))
            .height(Stretch(1.0))
            .width(Stretch(1.0));
    }

    fn slider_fill_view(
        cx: &mut Context,
        fill_start_delta_lens: impl Lens<Target = (f32, f32)>,
        modulation_start_delta_lens: impl Lens<Target = (f32, f32)>,
    ) {
        Element::new(cx)
            .class("fill")
            .width(Stretch(1.0)) // Swapped to width
            .bottom(fill_start_delta_lens.map(|(start_t, _)| Percentage(start_t * 100.0))) // Use bottom instead of left
            .height(fill_start_delta_lens.map(|(_, delta)| Percentage(delta * 100.0))) // Use height instead of width
            .hoverable(false);

        Element::new(cx)
            .class("fill")
            .class("fill--modulation")
            .width(Stretch(1.0))
            .visibility(modulation_start_delta_lens.map(|(_, delta)| *delta != 0.0))
            .height(modulation_start_delta_lens.map(|(_, delta)| Percentage(delta.abs() * 100.0)))
            .bottom(modulation_start_delta_lens.map(|(start_t, delta)| {
                if *delta < 0.0 {
                    Percentage((start_t + delta) * 100.0)
                } else {
                    Percentage(start_t * 100.0)
                }
            }))
            .hoverable(false);
    }

    fn slider_label_view<P: Param, L: Lens<Target = String>>(
        cx: &mut Context,
        param: &P,
        style: VerticalParamSliderStyle,
        display_value_lens: impl Lens<Target = String>,
        make_preview_value_lens: impl Fn(f32) -> L,
        label_override_lens: impl Lens<Target = Option<String>>,
    ) {
        let step_count = param.step_count();

        match (style, step_count) {
            (VerticalParamSliderStyle::CurrentStepLabeled { .. }, Some(step_count)) => {
                // Switched to VStack
                VStack::new(cx, |cx| {
                    // Reversed the range so the highest value sits at the top of the VStack
                    for value in (0..step_count + 1).rev() {
                        let normalized_value = value as f32 / step_count as f32;
                        let preview_lens = make_preview_value_lens(normalized_value);

                        Label::new(cx, preview_lens)
                            .class("value")
                            .class("value--multiple")
                            .child_space(Stretch(1.0))
                            .height(Stretch(1.0))
                            .width(Stretch(1.0))
                            .hoverable(false);
                    }
                })
                    .height(Stretch(1.0))
                    .width(Stretch(1.0))
                    .hoverable(false);
            }
            _ => {
                Binding::new(cx, label_override_lens, move |cx, label_override_lens| {
                    match label_override_lens.get(cx) {
                        Some(label_override) => Label::new(cx, &label_override),
                        None => Label::new(cx, display_value_lens),
                    }
                        .class("value")
                        .class("value--single")
                        .child_space(Stretch(1.0))
                        .height(Stretch(1.0))
                        .width(Stretch(1.0))
                        .hoverable(false);
                });
            }
        };
    }

    fn compute_fill_start_delta<P: Param>(
        style: VerticalParamSliderStyle,
        param: &P,
        current_value: f32,
    ) -> (f32, f32) {
        let default_value = param.default_normalized_value();
        let step_count = param.step_count();
        let draw_fill_from_default = matches!(style, VerticalParamSliderStyle::Centered)
            && step_count.is_none()
            && (0.45..=0.55).contains(&default_value);

        match style {
            VerticalParamSliderStyle::Centered if draw_fill_from_default => {
                let delta = (default_value - current_value).abs();
                (
                    default_value.min(current_value),
                    if delta >= 1e-3 { delta } else { 0.0 },
                )
            }
            VerticalParamSliderStyle::FromMidPoint => {
                let delta = (0.5 - current_value).abs();
                (
                    0.5_f32.min(current_value),
                    if delta >= 1e-3 { delta } else { 0.0 },
                )
            }
            VerticalParamSliderStyle::Centered | VerticalParamSliderStyle::FromBottom => {
                (0.0, current_value)
            }
            VerticalParamSliderStyle::CurrentStep { even: true }
            | VerticalParamSliderStyle::CurrentStepLabeled { even: true }
            if step_count.is_some() =>
                {
                    let step_count = step_count.unwrap() as f32;
                    let discrete_values = step_count + 1.0;
                    let previous_step = (current_value * step_count) / discrete_values;

                    (previous_step, discrete_values.recip())
                }
            VerticalParamSliderStyle::CurrentStep { .. } | VerticalParamSliderStyle::CurrentStepLabeled { .. } => {
                let previous_step = param.previous_normalized_step(current_value, false);
                let next_step = param.next_normalized_step(current_value, false);

                (
                    (previous_step + current_value) / 2.0,
                    ((next_step - current_value) + (current_value - previous_step)) / 2.0,
                )
            }
        }
    }

    fn compute_modulation_fill_start_delta<P: Param>(
        style: VerticalParamSliderStyle,
        param: &P,
    ) -> (f32, f32) {
        match style {
            VerticalParamSliderStyle::CurrentStep { .. } | VerticalParamSliderStyle::CurrentStepLabeled { .. } => {
                (0.0, 0.0)
            }
            VerticalParamSliderStyle::Centered
            | VerticalParamSliderStyle::FromMidPoint
            | VerticalParamSliderStyle::FromBottom => {
                let modulation_start = param.unmodulated_normalized_value();

                (
                    modulation_start,
                    param.modulated_normalized_value() - modulation_start,
                )
            }
        }
    }

    fn set_normalized_value_drag(&self, cx: &mut EventContext, normalized_value: f32) {
        let normalized_value = match (self.style, self.param_base.step_count()) {
            (
                VerticalParamSliderStyle::CurrentStep { even: true }
                | VerticalParamSliderStyle::CurrentStepLabeled { even: true },
                Some(step_count),
            ) => {
                let discrete_values = step_count as f32 + 1.0;
                let rounded_value = ((normalized_value * discrete_values) - 0.5).round();
                rounded_value / step_count as f32
            }
            _ => normalized_value,
        };

        self.param_base.set_normalized_value(cx, normalized_value);
    }
}

impl View for VerticalParamSlider {
    fn element(&self) -> Option<&'static str> {
        Some("vertical-param-slider")
    }

    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|param_slider_event, meta| match param_slider_event {
            VerticalParamSliderEvent::CancelTextInput => {
                self.text_input_active = false;
                cx.set_active(false);
                meta.consume();
            }
            VerticalParamSliderEvent::TextInput(string) => {
                if let Some(normalized_value) = self.param_base.string_to_normalized_value(string) {
                    self.param_base.begin_set_parameter(cx);
                    self.param_base.set_normalized_value(cx, normalized_value);
                    self.param_base.end_set_parameter(cx);
                }

                self.text_input_active = false;
                meta.consume();
            }
        });

        event.map(|window_event, meta| match window_event {
            WindowEvent::MouseDown(MouseButton::Left)
            | WindowEvent::MouseTripleClick(MouseButton::Left) => {
                if cx.modifiers().alt() {
                    self.text_input_active = true;
                    cx.set_active(true);
                } else if cx.modifiers().command() {
                    self.param_base.begin_set_parameter(cx);
                    self.param_base
                        .set_normalized_value(cx, self.param_base.default_normalized_value());
                    self.param_base.end_set_parameter(cx);
                } else if !self.text_input_active {
                    self.drag_active = true;
                    cx.capture();
                    cx.focus();
                    cx.set_active(true);

                    self.param_base.begin_set_parameter(cx);
                    if cx.modifiers().shift() {
                        self.granular_drag_status = Some(GranularDragStatus {
                            starting_y_coordinate: cx.mouse().cursory, // Swapped to cursory
                            starting_value: self.param_base.unmodulated_normalized_value(),
                        });
                    } else {
                        self.granular_drag_status = None;
                        let val = util::remap_current_entity_y_coordinate(cx, cx.mouse().cursory);
                        self.set_normalized_value_drag(cx, val);
                    }
                }

                meta.consume();
            }
            WindowEvent::MouseDoubleClick(MouseButton::Left)
            | WindowEvent::MouseDown(MouseButton::Right)
            | WindowEvent::MouseDoubleClick(MouseButton::Right)
            | WindowEvent::MouseTripleClick(MouseButton::Right) => {
                self.param_base.begin_set_parameter(cx);
                self.param_base
                    .set_normalized_value(cx, self.param_base.default_normalized_value());
                self.param_base.end_set_parameter(cx);

                meta.consume();
            }
            WindowEvent::MouseUp(MouseButton::Left) => {
                if self.drag_active {
                    self.drag_active = false;
                    cx.release();
                    cx.set_active(false);

                    self.param_base.end_set_parameter(cx);

                    meta.consume();
                }
            }
            WindowEvent::MouseMove(_x, y) => { // Track y instead of x
                if self.drag_active {
                    if cx.modifiers().shift() {
                        let granular_drag_status =
                            *self
                                .granular_drag_status
                                .get_or_insert_with(|| GranularDragStatus {
                                    starting_y_coordinate: *y,
                                    starting_value: self.param_base.unmodulated_normalized_value(),
                                });

                        let start_y =
                            util::remap_current_entity_y_t(cx, granular_drag_status.starting_value); // Swapped to Y mapping
                        let delta_y = ((*y - granular_drag_status.starting_y_coordinate)
                            * GRANULAR_DRAG_MULTIPLIER)
                            * cx.scale_factor();

                        let val = util::remap_current_entity_y_coordinate(cx, start_y + delta_y);
                        self.set_normalized_value_drag(cx, val);
                    } else {
                        self.granular_drag_status = None;

                        let val = util::remap_current_entity_y_coordinate(cx, *y);
                        self.set_normalized_value_drag(cx, val);
                    }
                }
            }
            WindowEvent::KeyUp(_, Some(Key::Shift)) => {
                if self.drag_active && self.granular_drag_status.is_some() {
                    self.granular_drag_status = None;
                    let val = util::remap_current_entity_y_coordinate(cx, cx.mouse().cursory);
                    self.param_base.set_normalized_value(cx, val);
                }
            }
            WindowEvent::MouseScroll(_scroll_x, scroll_y) if self.use_scroll_wheel => {
                self.scrolled_lines += scroll_y;

                if self.scrolled_lines.abs() >= 1.0 {
                    let use_finer_steps = cx.modifiers().shift();

                    if !self.drag_active {
                        self.param_base.begin_set_parameter(cx);
                    }

                    let mut current_value = self.param_base.unmodulated_normalized_value();

                    while self.scrolled_lines >= 1.0 {
                        current_value = self
                            .param_base
                            .next_normalized_step(current_value, use_finer_steps);
                        self.param_base.set_normalized_value(cx, current_value);
                        self.scrolled_lines -= 1.0;
                    }

                    while self.scrolled_lines <= -1.0 {
                        current_value = self
                            .param_base
                            .previous_normalized_step(current_value, use_finer_steps);
                        self.param_base.set_normalized_value(cx, current_value);
                        self.scrolled_lines += 1.0;
                    }

                    if !self.drag_active {
                        self.param_base.end_set_parameter(cx);
                    }
                }

                meta.consume();
            }
            _ => {}
        });
    }
}

pub trait VerticalParamSliderExt {
    fn disable_scroll_wheel(self) -> Self;
    fn set_style(self, style: VerticalParamSliderStyle) -> Self;
    fn with_label(self, value: impl Into<String>) -> Self;
}

impl VerticalParamSliderExt for Handle<'_, VerticalParamSlider> {
    fn disable_scroll_wheel(self) -> Self {
        self.modify(|param_slider: &mut VerticalParamSlider| param_slider.use_scroll_wheel = false)
    }

    fn set_style(self, style: VerticalParamSliderStyle) -> Self {
        self.modify(|param_slider: &mut VerticalParamSlider| param_slider.style = style)
    }

    fn with_label(self, value: impl Into<String>) -> Self {
        self.modify(|param_slider: &mut VerticalParamSlider| {
            param_slider.label_override = Some(value.into())
        })
    }
}
