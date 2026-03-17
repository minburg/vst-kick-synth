/*
 * Copyright (C) 2026 Marinus Burger
 */
//! Vizia model data and preset events for the editor.
//!
//! `Data` is the single source of truth for the UI; it is built once per
//! editor session and holds Arcs back to the audio-thread state.

use std::collections::HashSet;
use std::env;
use std::fs::{create_dir_all, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::{ParamEvent, RawParamEvent};
use crate::params::KickParams;
use crate::presets::{self, Preset, PresetOrigin};
// ── Model ───────────────────────────────────────────────────────────────────────
#[derive(Lens)]
pub struct Data {
    pub params: Arc<KickParams>,
    pub peak_meter_l: Arc<AtomicF32>,
    pub peak_meter_r: Arc<AtomicF32>,
    pub factory_presets: Vec<Preset>,
    pub user_presets: Vec<Preset>,
    pub user_preset_dir: PathBuf,
    pub bank_names: Vec<String>,
    pub selected_bank: usize,
    pub category_names: Vec<String>,
    pub selected_category: usize,
    pub preset_names: Vec<String>,
    pub selected_preset: usize,
    pub filtered_presets: Vec<Preset>,
}

impl Model for Data {
    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|preset_event, _| match preset_event {
            PresetEvent::SelectBank(index) => {
                self.selected_bank = *index;
                self.selected_category = 0;
                self.update_category_options();
            }
            PresetEvent::SelectCategory(index) => {
                self.selected_category = *index;
                self.refresh_preset_list();
            }
            PresetEvent::SelectPreset(index) => {
                self.selected_preset = *index;
                if let Some(preset) = self.filtered_presets.get(self.selected_preset) {
                    emit_params_events(cx, &self.params, preset);
                    nih_log!("Selected and loaded preset: {}", preset.name);
                }
            }
            PresetEvent::LoadSelection => {
                if let Some(preset) = self.filtered_presets.get(self.selected_preset) {
                    emit_params_events(cx, &self.params, preset);
                    nih_log!("Loaded preset: {}", preset.name);
                }
            }
            PresetEvent::SaveToFile => {
                let category = self
                    .category_names
                    .get(self.selected_category)
                    .cloned()
                    .unwrap_or_else(|| "User".to_string());
                let sanitized_category = sanitize_component(&category);
                let mut save_dir = self.user_preset_dir.join("user").join(&sanitized_category);
                if let Err(e) = create_dir_all(&save_dir) {
                    nih_log!("Failed to create preset directory {:?}: {}", save_dir, e);
                    save_dir = self.user_preset_dir.join("user");
                    let _ = create_dir_all(&save_dir);
                }

                nih_log!("Opening save dialog in: {:?}", save_dir);

                let default_name = self
                    .filtered_presets
                    .get(self.selected_preset)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "kick-synth-preset".to_string());
                let file_stem = sanitize_component(&default_name);
                let suggested_name = if file_stem.to_lowercase().ends_with(".json") {
                    file_stem
                } else {
                    format!("{}.json", file_stem)
                };
                let dialog_dir = save_dir.clone();
                let dialog_name = suggested_name.clone();
                cx.spawn(move |cxp| {
                    let path = rfd::FileDialog::new()
                        .set_directory(dialog_dir)
                        .set_file_name(&dialog_name)
                        .add_filter("JSON", &["json"])
                        .save_file();
                    // Some hosts might swallow standard output, so ensures we log result
                    if let Some(p) = &path {
                        nih_log!("User selected save path: {:?}", p);
                    } else {
                        nih_log!("User cancelled save dialog");
                    }
                    let _ = cxp.emit(PresetEvent::SaveToFileResult(path));
                });
            }
            PresetEvent::SaveToFileResult(path) => {
                if let Some(path) = path {
                    let mut preset = self.params.get_current_preset();
                    preset.origin = PresetOrigin::User;
                    if let Some(category) = self.category_names.get(self.selected_category) {
                        preset.categories = vec![category.clone()];
                    }
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        preset.name = stem.to_string();
                    }
                    match serde_json::to_string_pretty(&preset) {
                        Ok(json) => {
                            match File::create(&path) {
                                Ok(mut file) => {
                                    if let Err(e) = file.write_all(json.as_bytes()) {
                                        nih_log!("Failed to write preset to {:?}: {}", path, e);
                                    } else {
                                        nih_log!("Successfully saved preset to {:?}", path);
                                    }
                                }
                                Err(e) => nih_log!("Failed to create file {:?}: {}", path, e),
                            }
                        }
                        Err(e) => nih_log!("Failed to serialize preset: {}", e),
                    }
                    self.reload_user_presets();
                }
            }
            PresetEvent::LoadFromFile => {
                let mut load_dir = self.user_preset_dir.join("user");
                if !load_dir.exists() {
                     load_dir = self.user_preset_dir.clone();
                }
                if let Err(e) = create_dir_all(&load_dir) {
                     nih_log!("Failed to create dir {:?}: {}", load_dir, e);
                }

                nih_log!("Opening load dialog in: {:?}", load_dir);

                let dialog_dir = load_dir.clone();
                cx.spawn(move |cxp| {
                    let path = rfd::FileDialog::new()
                        .set_directory(dialog_dir)
                        .add_filter("JSON", &["json"])
                        .pick_file();
                    if let Some(p) = &path {
                         nih_log!("User selected file to load: {:?}", p);
                    } else {
                         nih_log!("User cancelled load dialog");
                    }
                    let _ = cxp.emit(PresetEvent::LoadFromFileResult(path));
                });
            }
            PresetEvent::LoadFromFileResult(path) => {
                if let Some(path) = path {
                    match File::open(&path) {
                        Ok(mut file) => {
                            let mut json = String::new();
                            if file.read_to_string(&mut json).is_ok() {
                                match serde_json::from_str::<Preset>(&json) {
                                    Ok(preset) => {
                                        emit_params_events(cx, &self.params, &preset);
                                        nih_log!("Loaded preset from file: {:?}", path);
                                    }
                                    Err(e) => nih_log!("Failed to deserialize preset from {:?}: {}", path, e),
                                }
                            }
                        }
                        Err(e) => nih_log!("Failed to open preset file {:?}: {}", path, e),
                    }
                }
            }
        });
    }
}
// ── Preset events ───────────────────────────────────────────────────────────────
pub enum PresetEvent {
    SelectBank(usize),
    SelectCategory(usize),
    SelectPreset(usize),
    LoadSelection,
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
    emit(cx, &params.filter_wet_dry, preset.filter_wet_dry);
}
// ── Data constructor ─────────────────────────────────────────────────────────────
impl Data {
    pub fn new(
        params: Arc<KickParams>,
        peak_meter_l: Arc<AtomicF32>,
        peak_meter_r: Arc<AtomicF32>,
    ) -> Self {
        let user_preset_dir = default_user_preset_dir();
        let user_root = user_preset_dir.join("user");
        if let Err(e) = create_dir_all(&user_root) {
            nih_log!("Failed to create user preset root: {}", e);
        }
        let factory_presets = presets::get_factory_presets();
        let mut data = Self {
            params,
            peak_meter_l,
            peak_meter_r,
            factory_presets,
            user_presets: load_user_presets(&user_root),
            user_preset_dir,
            bank_names: vec!["Factory".to_string(), "User".to_string()],
            selected_bank: 0,
            category_names: Vec::new(),
            selected_category: 0,
            preset_names: Vec::new(),
            selected_preset: 0,
            filtered_presets: Vec::new(),
        };
        data.update_category_options();
        data
    }
    fn update_category_options(&mut self) {
        let mut categories = self.compute_category_names(self.selected_bank);
        if categories.is_empty() {
            categories.push("General".to_string());
        }
        self.category_names = categories;
        if self.selected_category >= self.category_names.len() {
            self.selected_category = 0;
        }
        self.refresh_preset_list();
    }
    fn compute_category_names(&self, bank_index: usize) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();

        // For the user bank, also include names of any subdirectories so that
        // categories appear even before the first preset is saved into them
        // (useful when the user manually creates category folders).
        if bank_index == 1 {
            let user_dir = self.user_preset_dir.join("user");
            if let Ok(read_dir) = user_dir.read_dir() {
                let mut dirs: Vec<_> = read_dir.flatten()
                    .filter(|e| e.path().is_dir())
                    .collect();
                dirs.sort_by_key(|e| e.path());
                for entry in dirs {
                    if let Some(name) = entry.file_name().to_str() {
                        if seen.insert(name.to_string()) {
                            result.push(name.to_string());
                        }
                    }
                }
            }
        }

        for preset in self.presets_for_bank(bank_index) {
            let category = preset
                .categories
                .get(0)
                .cloned()
                .unwrap_or_else(|| "General".to_string());
            if seen.insert(category.clone()) {
                result.push(category);
            }
        }
        result
    }
    fn presets_for_bank(&self, bank_index: usize) -> &[Preset] {
        match bank_index {
            0 => &self.factory_presets,
            1 => &self.user_presets,
            _ => &[],
        }
    }
    fn refresh_preset_list(&mut self) {
        let filtered = self.presets_for_current_selection();
        self.filtered_presets = filtered.clone();
        if filtered.is_empty() {
            self.preset_names = vec!["(no presets)".to_string()];
            self.selected_preset = 0;
            return;
        }
        self.preset_names = filtered.iter().map(|p| p.name.clone()).collect();
        if self.selected_preset >= self.preset_names.len() {
            self.selected_preset = 0;
        }
    }
    fn presets_for_current_selection(&self) -> Vec<Preset> {
        let bank_presets = self.presets_for_bank(self.selected_bank);
        if bank_presets.is_empty() {
            return Vec::new();
        }
        if let Some(category_name) = self.category_names.get(self.selected_category) {
            let filtered: Vec<Preset> = bank_presets
                .iter()
                .filter(|preset| {
                    preset
                        .categories
                        .get(0)
                        .map(|cat| cat == category_name)
                        .unwrap_or_else(|| category_name == "General")
                })
                .cloned()
                .collect();
            if !filtered.is_empty() {
                return filtered;
            }
        }
        bank_presets.to_vec()
    }
    fn reload_user_presets(&mut self) {
        let user_root = self.user_preset_dir.join("user");
        self.user_presets = load_user_presets(&user_root);
        if self.selected_bank == 1 {
            self.update_category_options();
        }
    }
}
// ── Helpers ─────────────────────────────────────────────────────────────────────
fn sanitize_component(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "default".to_string();
    }
    let mut sanitized = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else if c.is_whitespace() {
                '_'
            } else if c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    sanitized = sanitized.trim_matches('_').to_string();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}
fn default_user_preset_dir() -> PathBuf {
    let base = if cfg!(target_os = "windows") {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("USERPROFILE").map(PathBuf::from).map(|p| p.join("AppData").join("Local"))
            })
            .unwrap_or_else(|| PathBuf::from("."))
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join("Library").join("Application Support"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME").map(PathBuf::from).map(|p| p.join(".local").join("share"))
            })
            .unwrap_or_else(|| PathBuf::from("."))
    };
    base.join("ConvolutionDEV").join("KickSynth").join("Presets")
}
fn load_user_presets(root: &Path) -> Vec<Preset> {
    let mut presets = Vec::new();
    if !root.exists() {
        return presets;
    }
    scan_user_preset_dir(root, root, &mut presets);
    presets
}
fn scan_user_preset_dir(dir: &Path, root: &Path, presets: &mut Vec<Preset>) {
    if let Ok(read_dir) = dir.read_dir() {
        let mut entries: Vec<_> = read_dir.flatten().collect();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                scan_user_preset_dir(&path, root, presets);
                continue;
            }
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("json"))
                .unwrap_or(false)
            {
                match File::open(&path) {
                    Ok(mut file) => {
                        let mut json = String::new();
                        if file.read_to_string(&mut json).is_err() {
                            nih_log!("Failed to read user preset {:?}", path);
                            continue;
                        }
                        match serde_json::from_str::<Preset>(&json) {
                            Ok(mut preset) => {
                                preset.origin = PresetOrigin::User;
                                if preset.categories.is_empty() {
                                    if let Ok(relative) = path.strip_prefix(root) {
                                        if let Some(folder) = relative.components().next() {
                                            if let Some(name) = folder.as_os_str().to_str() {
                                                preset.categories = vec![name.to_string()];
                                            }
                                        }
                                    }
                                }
                                presets.push(preset);
                            }
                            Err(e) => nih_log!("Failed to parse user preset {:?}: {}", path, e),
                        }
                    }
                    Err(e) => nih_log!("Failed to open user preset {:?}: {}", path, e),
                }
            }
        }
    }
}
