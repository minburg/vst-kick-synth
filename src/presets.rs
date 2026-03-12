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
    pub nam_output_gain: f32,
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
            nam_output_gain: 0.0,
            nam_model: NamModel::PhilipsEL3541D,
        }
    }
}

pub fn get_factory_presets() -> Vec<Preset> {
    vec![
        Preset {
            name: "Classic 808".to_string(),
            tune: 50.0,
            sweep: 150.0,
            pitch_decay: 60.0,
            decay: 400.0,
            sustain: 0.0,
            release: 200.0,
            bass_synth_mode: true,
            ..Preset::default()
        },
        Preset {
            name: "Deep House Thump".to_string(),
            tune: 42.0,
            sweep: 300.0,
            pitch_decay: 80.0,
            drive: 0.2,
            drive_model: 2,
            decay: 250.0,
            sustain: 0.2,
            ..Preset::default()
        },
        Preset {
            name: "Aggressive Techno".to_string(),
            tune: 48.0,
            sweep: 600.0,
            drive: 0.6,
            drive_model: 5,
            corrosion_amount: 0.4,
            corrosion_frequency: 0.7,
            ..Preset::default()
        },
        Preset {
            name: "Dubstep Weight".to_string(),
            tune: 38.0,
            sweep: 200.0,
            pitch_decay: 120.0,
            drive: 0.4,
            drive_model: 4,
            tex_amt: 0.3,
            tex_type: 3,
            ..Preset::default()
        },
        Preset {
            name: "Vintage Tape".to_string(),
            drive: 0.5,
            drive_model: 1,
            nam_active: true,
            nam_model: NamModel::JH24,
            nam_input_gain: 3.0,
            nam_output_gain: -3.0,
            ..Preset::default()
        },
        Preset {
            name: "Dirty Tube".to_string(),
            drive: 0.7,
            drive_model: 3,
            nam_active: true,
            nam_model: NamModel::CultureVulture,
            nam_input_gain: 6.0,
            nam_output_gain: -6.0,
            ..Preset::default()
        },
        Preset {
            name: "Industrial Crunch".to_string(),
            drive: 0.8,
            drive_model: 5,
            corrosion_amount: 0.6,
            corrosion_noise_blend: 0.5,
            tex_amt: 0.5,
            tex_type: 1,
            ..Preset::default()
        },
        Preset {
            name: "Soft Pop".to_string(),
            tune: 55.0,
            sweep: 100.0,
            drive: 0.1,
            decay: 150.0,
            release: 50.0,
            ..Preset::default()
        },
        Preset {
            name: "Punchy Rock".to_string(),
            tune: 60.0,
            sweep: 400.0,
            pitch_decay: 40.0,
            drive: 0.3,
            drive_model: 1,
            tex_amt: 0.1,
            tex_type: 2,
            ..Preset::default()
        },
        Preset {
            name: "Glitchy Zap".to_string(),
            tune: 80.0,
            sweep: 1000.0,
            pitch_decay: 20.0,
            tex_amt: 0.8,
            tex_type: 6,
            tex_decay: 30.0,
            ..Preset::default()
        },
        Preset {
            name: "Vinyl Soul".to_string(),
            drive: 0.2,
            tex_amt: 0.4,
            tex_type: 5,
            tex_tone: 0.3,
            drive_model: 1,
            ..Preset::default()
        },
    ]
}
