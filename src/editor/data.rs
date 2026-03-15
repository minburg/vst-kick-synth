/*
 * Copyright (C) 2026 Marinus Burger
 */

//! Vizia model data and preset events for the editor.
//!
//! `Data` is the single source of truth for the UI; it is built once per
//! editor session and holds Arcs back to the audio-thread state.

use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::{ParamEvent, RawParamEvent};

use crate::params::KickParams;
use crate::presets::{self, Preset};

// ── Model ───────────────────────────────────────────────────────────────────────

#[derive(Lens)]
pub struct Data {
    pub params: Arc<KickParams>,
    pub peak_meter_l: Arc<AtomicF32>,
    pub peak_meter_r: Arc<AtomicF32>,
    pub factory_presets: Vec<Preset>,
    pub selected_preset: usize,
}

impl Model for Data {
    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|preset_event, _| match preset_event {
            PresetEvent::LoadFactory(idx) => {
                let preset = &self.factory_presets[*idx];
                self.selected_preset = *idx;
                emit_params_events(cx, &self.params, preset);
            }
            PresetEvent::SaveToFile => {
                cx.spawn(|cxp| {
                    let path = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .save_file();
                    let _ = cxp.emit(PresetEvent::SaveToFileResult(path));
                });
            }
            PresetEvent::SaveToFileResult(path) => {
                if let Some(path) = path {
                    let preset = self.params.get_current_preset();
                    match serde_json::to_string_pretty(&preset) {
                        Ok(json) => {
                            if let Ok(mut file) = File::create(path) {
                                let _ = file.write_all(json.as_bytes());
                            }
                        }
                        Err(e) => nih_log!("Failed to serialize preset: {}", e),
                    }
                }
            }
            PresetEvent::LoadFromFile => {
                cx.spawn(|cxp| {
                    let path = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file();
                    let _ = cxp.emit(PresetEvent::LoadFromFileResult(path));
                });
            }
            PresetEvent::LoadFromFileResult(path) => {
                if let Some(path) = path {
                    match File::open(path) {
                        Ok(mut file) => {
                            let mut json = String::new();
                            if file.read_to_string(&mut json).is_ok() {
                                match serde_json::from_str::<Preset>(&json) {
                                    Ok(preset) => {
                                        emit_params_events(cx, &self.params, &preset);
                                    }
                                    Err(e) => nih_log!("Failed to deserialize preset: {}", e),
                                }
                            }
                        }
                        Err(e) => nih_log!("Failed to open preset file: {}", e),
                    }
                }
            }
        });
    }
}

// ── Preset events ───────────────────────────────────────────────────────────────

pub enum PresetEvent {
    LoadFactory(usize),
    SaveToFile,
    SaveToFileResult(Option<PathBuf>),
    LoadFromFile,
    LoadFromFileResult(Option<PathBuf>),
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// Emit the full set of nih-plug parameter-change events required to apply
/// every field in `preset` to `params`.
pub fn emit_params_events(cx: &mut EventContext, params: &Arc<KickParams>, preset: &Preset) {
    fn emit<P: Param + Sync + 'static>(cx: &mut EventContext, param: &P, value: P::Plain)
    where
        P::Plain: Send + Clone,
    {
        let ptr = param.as_ptr();
        // transmuting to 'static is safe here because we know params outlives the editor
        let param_static: &'static P = unsafe { std::mem::transmute(param) };

        let normalized = param.preview_normalized(value.clone());

        cx.emit(ParamEvent::<P>::BeginSetParameter(param_static));
        cx.emit(RawParamEvent::BeginSetParameter(ptr));
        cx.emit(ParamEvent::<P>::SetParameter(param_static, value));
        cx.emit(RawParamEvent::SetParameterNormalized(ptr, normalized));
        cx.emit(ParamEvent::<P>::EndSetParameter(param_static));
        cx.emit(RawParamEvent::EndSetParameter(ptr));
    }

    emit(cx, &params.tune, preset.tune);
    emit(cx, &params.waveform, preset.waveform);
    emit(cx, &params.sweep, preset.sweep);
    emit(cx, &params.pitch_decay, preset.pitch_decay);
    emit(cx, &params.drive, preset.drive);
    emit(cx, &params.drive_model, preset.drive_model);
    emit(cx, &params.tex_amt, preset.tex_amt);
    emit(cx, &params.tex_decay, preset.tex_decay);
    emit(cx, &params.tex_variation, preset.tex_variation);
    emit(cx, &params.analog_variation, preset.analog_variation);
    emit(cx, &params.tex_type, preset.tex_type);
    emit(cx, &params.tex_tone, preset.tex_tone);
    emit(cx, &params.attack, preset.attack);
    emit(cx, &params.decay, preset.decay);
    emit(cx, &params.sustain, preset.sustain);
    emit(cx, &params.release, preset.release);
    emit(cx, &params.corrosion_frequency, preset.corrosion_frequency);
    emit(cx, &params.corrosion_width, preset.corrosion_width);
    emit(
        cx,
        &params.corrosion_noise_blend,
        preset.corrosion_noise_blend,
    );
    emit(cx, &params.corrosion_stereo, preset.corrosion_stereo);
    emit(cx, &params.corrosion_amount, preset.corrosion_amount);
    emit(cx, &params.bass_synth_mode, preset.bass_synth_mode);
    emit(cx, &params.nam_active, preset.nam_active);
    emit(cx, &params.nam_input_gain, preset.nam_input_gain);
    emit(cx, &params.output_gain, preset.output_gain);
    emit(cx, &params.nam_model, preset.nam_model);

    // Filter params
    emit(cx, &params.filter_active, preset.filter_active);
    emit(cx, &params.filter_type, preset.filter_type);
    emit(cx, &params.filter_position, preset.filter_position);
    emit(cx, &params.filter_cutoff, preset.filter_cutoff);
    emit(cx, &params.filter_resonance, preset.filter_resonance);
    emit(cx, &params.filter_env_amount, preset.filter_env_amount);
    emit(cx, &params.filter_env_attack, preset.filter_env_attack);
    emit(cx, &params.filter_env_decay, preset.filter_env_decay);
    emit(cx, &params.filter_env_sustain, preset.filter_env_sustain);
    emit(cx, &params.filter_env_release, preset.filter_env_release);
    emit(cx, &params.filter_drive, preset.filter_drive);
    emit(cx, &params.filter_key_track, preset.filter_key_track);
}

// ── Data constructor ─────────────────────────────────────────────────────────────

impl Data {
    pub fn new(
        params: Arc<KickParams>,
        peak_meter_l: Arc<AtomicF32>,
        peak_meter_r: Arc<AtomicF32>,
    ) -> Self {
        Self {
            params,
            peak_meter_l,
            peak_meter_r,
            factory_presets: presets::get_factory_presets(),
            selected_preset: 14, // Vinyl Soul
        }
    }
}
