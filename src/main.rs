use nih_plug::prelude::*;
use kick_synth::KickSynth;

fn main() {
    nih_export_standalone::<KickSynth>();
}
