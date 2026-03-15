/*
 * Copyright (C) 2026 Marinus Burger
 */

//! Generic UI widget builders shared across multiple editor panels.
//!
//! These helpers wrap common nih-plug-vizia patterns for toggle buttons and
//! momentary (mouse-down/up) trigger buttons, keeping panel code declarative.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::{ParamEvent, RawParamEvent};

use crate::params::KickParams;

// ── Toggle button ───────────────────────────────────────────────────────────────

/// A styled VStack that acts as a boolean toggle for `param`.
///
/// Clicking toggles the parameter value between 0 and 1.
pub fn create_toggle_button<'a, L, F>(
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

// ── Text (trigger) button ───────────────────────────────────────────────────────

/// A styled VStack that fires a momentary trigger on mouse-down and releases
/// on mouse-up.
///
/// Increments `gui_trigger` / `gui_release` atomics so the audio thread can
/// detect the event even at high buffer sizes.
pub fn create_text_button<'a, L, F>(
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
