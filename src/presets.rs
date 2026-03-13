use serde::{Serialize, Deserialize};
use crate::NamModel;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Preset {
    pub name: String,
    pub tune: f32,
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
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            tune: 44.0,
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
