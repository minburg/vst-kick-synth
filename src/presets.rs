use serde::{Deserialize, Serialize};
use crate::{FilterPosition, FilterType, NamModel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresetOrigin {
    #[serde(rename = "Factory")]
    Factory,
    #[serde(rename = "User")]
    User,
}

impl Default for PresetOrigin {
    fn default() -> Self {
        PresetOrigin::Factory
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub origin: PresetOrigin,
    pub tune: f32,
    pub waveform: i32,
    pub sweep: f32,
    pub pitch_decay: f32,
    pub drive: f32,
    pub drive_model: i32,
    pub tex_amt: f32,
    pub tex_decay: f32,
    pub tex_variation: f32,
    pub analog_variation: f32,
    pub tex_type: i32,
    pub tex_tone: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub corrosion_frequency: f32,
    pub corrosion_width: f32,
    pub corrosion_noise_blend: f32,
    pub corrosion_stereo: f32,
    pub corrosion_amount: f32,
    pub bass_synth_mode: bool,
    pub nam_active: bool,
    pub nam_input_gain: f32,
    #[serde(default)]
    pub output_gain: f32,
    pub nam_model: NamModel,
    #[serde(default)]
    pub categories: Vec<String>,
    // Filter — all fields have #[serde(default)] so existing presets without
    // these keys simply use the defaults (filter off, LP24, PostNam, etc.).
    #[serde(default)]
    pub filter_active: bool,
    #[serde(default)]
    pub filter_type: FilterType,
    #[serde(default)]
    pub filter_position: FilterPosition,
    #[serde(default = "default_filter_cutoff")]
    pub filter_cutoff: f32,
    #[serde(default = "default_filter_resonance")]
    pub filter_resonance: f32,
    #[serde(default = "default_filter_env_amount")]
    pub filter_env_amount: f32,
    #[serde(default = "default_filter_env_attack")]
    pub filter_env_attack: f32,
    #[serde(default = "default_filter_env_decay")]
    pub filter_env_decay: f32,
    #[serde(default)]
    pub filter_env_sustain: f32,
    #[serde(default = "default_filter_env_release")]
    pub filter_env_release: f32,
    /// true = trigger mode (fire-and-forget, best for kick drums);
    /// false = gate mode (sustain held until note-off).
    #[serde(default = "default_filter_env_trigger")]
    pub filter_env_trigger: bool,
    #[serde(default)]
    pub filter_drive: f32,
    #[serde(default)]
    pub filter_key_track: f32,
}

fn default_filter_cutoff()      -> f32 { 2000.0 }
fn default_filter_resonance()   -> f32 { 0.3 }
fn default_filter_env_amount()  -> f32 { 2.0 }
fn default_filter_env_attack()  -> f32 { 5.0 }
fn default_filter_env_decay()   -> f32 { 300.0 }
fn default_filter_env_release() -> f32 { 200.0 }
fn default_filter_env_trigger() -> bool { true }

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            origin: PresetOrigin::Factory,
            tune: 44.0,
            waveform: 1,
            sweep: 239.0,
            pitch_decay: 100.0,
            drive: 0.33,
            drive_model: 1,
            tex_amt: 0.2,
            tex_decay: 80.0,
            tex_variation: 0.0,
            analog_variation: 0.0,
            tex_type: 1,
            tex_tone: 0.5,
            attack: 0.1,
            decay: 153.0,
            sustain: 0.44,
            release: 128.0,
            corrosion_frequency: 0.5,
            corrosion_width: 0.5,
            corrosion_noise_blend: 1.0,
            corrosion_stereo: 0.0,
            corrosion_amount: 0.0,
            bass_synth_mode: false,
            nam_active: false,
            nam_input_gain: 0.0,
            output_gain: 0.0,
            nam_model: NamModel::PhilipsEL3541D,
            filter_active: false,
            filter_type: FilterType::LP24,
            filter_position: FilterPosition::PostNam,
            filter_cutoff: 1500.0,
            filter_resonance: 0.16,
            filter_env_amount: 4.0,
            filter_env_attack: 0.1,
            filter_env_decay: 230.0,
            filter_env_sustain: 0.0,
            filter_env_release: 200.0,
            filter_env_trigger: true,
            filter_drive: 0.0,
            filter_key_track: 0.0,
            categories: Vec::new(),
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/presets_generated.rs"));

pub fn get_factory_presets() -> Vec<Preset> {
    PRESET_JSONS
        .iter()
        .map(|json| serde_json::from_str(json).expect("Failed to parse factory preset"))
        .collect()
}
