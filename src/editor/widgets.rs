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

// ── DebugWrapper ────────────────────────────────────────────────────────────────

/// A transparent wrapper view that logs mouse/focus events via `nih_log!`.
/// Used in place of a bare `VStack` so we can diagnose Cubase hit-testing issues.
pub struct DebugWrapper {
    name: String,
}

impl DebugWrapper {
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

// ── Toggle button ───────────────────────────────────────────────────────────────

/// A styled [`DebugWrapper`] that acts as a boolean toggle for `param`.
///
/// Clicking toggles the parameter value between 0 and 1.
///
/// Uses a 20 ms timer to close the parameter gesture *after* the value has been
/// set, which is required for Cubase on macOS to reliably register the change.
pub fn create_toggle_button<'a, L, F>(
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
    .on_press(move |cx| {
        cx.focus();
        cx.set_active(true);

        let param = selector(&params_arc);
        let new_value = !param.value();
        let normalized = if new_value { 1.0 } else { 0.0 };

        let ptr = param.as_ptr();
        // SAFETY: `param` is a reference into `params_arc` which is kept alive
        // for the lifetime of the plugin. Transmuting to `'static` is safe here
        // because the timer callback only fires while the editor is open and the
        // Arc is still alive.
        let param_static: &'static BoolParam = unsafe { std::mem::transmute(param) };

        // Phase 1: open the gesture and set the new value immediately.
        cx.emit(ParamEvent::BeginSetParameter(param_static));
        cx.emit(RawParamEvent::BeginSetParameter(ptr));
        cx.emit(ParamEvent::SetParameterNormalized(param_static, normalized));
        cx.emit(RawParamEvent::SetParameterNormalized(ptr, normalized));

        // Phase 2: close the gesture after 20 ms so Cubase has time to
        // observe the value change before the gesture ends.
        let duration = std::time::Duration::from_millis(20);
        cx.add_timer(duration, Some(duration), move |cx, action| {
            if let TimerAction::Stop = action {
                cx.emit(ParamEvent::EndSetParameter(param_static));
                cx.emit(RawParamEvent::EndSetParameter(ptr));
                nih_log!("TIMER: Gesture closed for {}", label_text);
            }
        });

        nih_log!("TIMER: Gesture opened for {} (20 ms duration)", label_text);
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
