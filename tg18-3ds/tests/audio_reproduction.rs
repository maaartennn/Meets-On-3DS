//! Reproduce the pacing regression with synthetic timer events and the real core.
#![allow(dead_code)]
#[path = "../src/audio.rs"]
mod audio;
mod legacy_audio;

fn main() {
    let mut legacy = legacy_audio::LegacyAudio::new();
    let mut native = audio::NativeAudio::new(0.5);
    let mut old_output = legacy_audio::OutputModel::new();
    let mut revised_output = legacy_audio::OutputModel::new();
    let mut fixed_output = legacy_audio::OutputModel::new();
    let mut wall = 0.0;
    let mut native_old = 0;
    let mut generated_old = 0;
    for frame in 0..30 {
        let dt = if frame == 2 { 0.1 } else { 1.0 / 60.0 };
        wall += dt;
        let before = frame as f64 / 60.0;
        let now = (frame + 1) as f64 / 60.0;
        let events = if frame == 0 {
            vec![(0.0, 2793.0, 0.5)]
        } else if frame == 3 {
            vec![(0.064, 0.0, 0.5)]
        } else {
            vec![]
        };
        let (n_emu, pcm) = legacy.render(now, dt, &events);
        native_old += n_emu;
        generated_old += pcm.len();
        old_output.accept(wall, 735);
        revised_output.accept(wall, pcm.len());
        native.render(&events, before, now, false);
        while native.chunk_len() > 0 {
            if !fixed_output.accept(wall, native.chunk_len()) {
                break;
            }
            native.commit_chunk();
        }
    }
    println!("Synthetic trace: 30 emulation frames; one 100 ms wall-clock stall.");
    println!(
        "Legacy synthesis: native={native_old}, generated={generated_old}, extra_ms={:.3}",
        (generated_old - native_old) as f64 / 44100.0 * 1000.0
    );
    println!(
        "v49 fixed-count playback: dropped_chunks={}",
        old_output.dropped
    );
    println!(
        "Revised full-length playback: dropped_chunks={}",
        revised_output.dropped
    );
    println!(
        "Correction: generated={}, queued={}, overrun={}, dropped_chunks={}",
        native.stats.generated_samples,
        native.stats.queued_samples,
        native.stats.overrun_samples,
        fixed_output.dropped
    );
    println!("This is a host duration/occupancy model, not a 3DS speaker recording.");
}
