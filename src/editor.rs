/*
 * Copyright (C) 2026 Marinus Burger
 */

//! Editor entry point.
//!
//! This module wires together all editor sub-modules and exposes the two
//! functions called by the plugin host: `default_state` and `create`.
//!
//! Layout and widget logic lives in the submodules below — this file stays
//! intentionally thin so the overall structure is easy to navigate.

use nih_plug::prelude::*;
use nih_plug_vizia::assets::register_noto_sans_light;
use nih_plug_vizia::vizia::image::load_from_memory;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::Arc;

use crate::params::KickParams;

// ── Submodules ──────────────────────────────────────────────────────────────────

pub mod data;
pub mod my_peak_meter;
pub mod panels;
pub mod param_knob;
pub mod single_knob;
pub mod util;
pub mod vertical_param_slider;
pub mod widgets;

// ── Embedded assets ─────────────────────────────────────────────────────────────

pub const ORBITRON_TTF: &[u8] = include_bytes!("resource/fonts/Orbitron-Regular.ttf");
pub const COMFORTAA_LIGHT_TTF: &[u8] = include_bytes!("resource/fonts/Comfortaa-Light.ttf");
pub const COMFORTAA: &str = "Comfortaa";

const BG_IMAGE_BYTES: &[u8] =
    include_bytes!("resource/images/kick_background_tint_cropped.png");
const POTI_3_IMAGE_BYTES: &[u8] =
    include_bytes!("resource/images/poti_3_fixed_small.png");
const POTI_1_IMAGE_BYTES: &[u8] =
    include_bytes!("resource/images/poti_1_fixed_small.png");
const INSTA_ICON_BYTES: &[u8] =
    include_bytes!("resource/images/instagram_icon.png");
const SPOTIFY_ICON_BYTES: &[u8] =
    include_bytes!("resource/images/spotify_icon.png");

// ── Public API ──────────────────────────────────────────────────────────────────

/// Build and return the editor instance.
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

        load_image_asset(cx, "background_image.png", BG_IMAGE_BYTES);
        load_image_asset(cx, "poti_3_fixed_small.png", POTI_3_IMAGE_BYTES);
        load_image_asset(cx, "poti_1_fixed_small.png", POTI_1_IMAGE_BYTES);
        load_image_asset(cx, "insta.png", INSTA_ICON_BYTES);
        load_image_asset(cx, "spotify.png", SPOTIFY_ICON_BYTES);

        if let Err(e) = cx.add_stylesheet(include_style!("/src/resource/style.css")) {
            nih_log!("CSS Error: {:?}", e);
        }

        data::Data::new(params.clone(), peak_meter_l.clone(), peak_meter_r.clone())
            .build(cx);

        panels::build_main_layout(&params, cx);
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// Load an embedded image into the Vizia context, logging on failure.
#[inline]
fn load_image_asset(cx: &mut Context, name: &str, bytes: &[u8]) {
    match load_from_memory(bytes) {
        Ok(img) => cx.load_image(name, img, ImageRetentionPolicy::Forever),
        Err(e) => nih_error!("Failed to load image '{}': {}", name, e),
    }
}

