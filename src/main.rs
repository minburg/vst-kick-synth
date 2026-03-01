use nih_plug::prelude::*;
use kick_synth::KickSynth;
fn main() {
    let mut args: Vec<String> = std::env::args().collect();

    // If no arguments were provided (other than the executable name),
    // inject defaults to prevent WASAPI initialization errors on standard interfaces.
    if args.len() == 1 {
        args.push("--sample-rate".to_string());
        args.push("44100".to_string());
        args.push("--period-size".to_string());
        args.push("1024".to_string());
    }

    nih_export_standalone_with_args::<KickSynth, _>(args.into_iter());
}
