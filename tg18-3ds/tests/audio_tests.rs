use super::{audio, legacy_audio};
use audio::{NativeAudio, CHUNK_SAMPLES, SAMPLE_RATE};
use tg18::sound::ToneSynth;

fn drain(audio: &mut NativeAudio, output: &mut Vec<i16>) {
    while audio.chunk_len() > 0 {
        output.extend(audio.chunk());
        audio.commit_chunk();
    }
}

fn trace() -> Vec<(f64, f64, f64)> {
    vec![
        (0.0, 2793.0, 0.5),
        (0.064, 0.0, 0.5),
        (0.192, 2350.0, 0.5),
        (0.448, 0.0, 0.5),
    ]
}

#[test]
fn legacy_count_change_exposes_excess_duration_and_queue_drops() {
    let mut v49 = legacy_audio::LegacyAudio::new();
    let mut revised = legacy_audio::LegacyAudio::new();
    let mut old_output = legacy_audio::OutputModel::new();
    let mut revised_output = legacy_audio::OutputModel::new();
    let mut wall = 0.0;
    let mut native = 0;
    let mut generated = 0;
    for frame in 0..30 {
        let dt = if frame == 2 { 0.1 } else { 1.0 / 60.0 };
        wall += dt;
        let now = (frame + 1) as f64 / 60.0;
        let events = if frame == 0 {
            vec![(0.0, 2793.0, 0.5)]
        } else if frame == 3 {
            vec![(0.064, 0.0, 0.5)]
        } else {
            vec![]
        };
        let (n_emu, old_pcm) = v49.render(now, dt, &events);
        let (_, revised_pcm) = revised.render(now, dt, &events);
        assert_eq!(
            old_pcm, revised_pcm,
            "capacity/count/flush did not change synthesis"
        );
        native += n_emu;
        generated += revised_pcm.len();
        old_output.accept(wall, 735);
        revised_output.accept(wall, revised_pcm.len());
    }
    assert_eq!(native, 22049);
    assert_eq!(generated, 39609);
    assert_eq!(old_output.dropped, 0);
    assert_eq!(revised_output.dropped, 14);
}

#[test]
fn native_pcm_matches_the_actual_core_one_shot_synthesizer() {
    let events = trace();
    let mut reference = ToneSynth::new(SAMPLE_RATE, 0.5);
    let mut expected = Vec::new();
    reference.render(&events, 0.0, 0.5, 22050, &mut expected);
    let mut audio = NativeAudio::new(0.5);
    let mut actual = Vec::new();
    let mut event = 0;
    for frame in 0..30 {
        let before = frame as f64 / 60.0;
        let now = (frame + 1) as f64 / 60.0;
        let end = events.partition_point(|e| e.0 <= now);
        audio.render(&events[event..end], before, now, false);
        event = end;
        drain(&mut audio, &mut actual);
    }
    assert_eq!(
        actual, expected,
        "pitch, duty, phase and note durations must match native core PCM"
    );
    assert_eq!(audio.stats.generated_samples, 22050);
    assert_eq!(audio.stats.queued_samples, 22050);
    assert_eq!(audio.stats.overrun_samples, 0);
}

#[test]
fn wall_stall_has_no_stretch_tail_or_dropped_native_chunks() {
    let mut audio = NativeAudio::new(0.5);
    let mut output = legacy_audio::OutputModel::new();
    let mut wall = 0.0;
    let mut count = 0;
    for frame in 0..30 {
        wall += if frame == 2 { 0.1 } else { 1.0 / 60.0 };
        let events = if frame == 0 {
            vec![(0.0, 2793.0, 0.5)]
        } else {
            vec![]
        };
        audio.render(
            &events,
            frame as f64 / 60.0,
            (frame + 1) as f64 / 60.0,
            false,
        );
        while audio.chunk_len() > 0 {
            assert_eq!(audio.chunk_len(), 735);
            if !output.accept(wall, audio.chunk_len()) {
                break;
            }
            audio.commit_chunk();
            count += 1;
        }
    }
    assert_eq!(count, 30);
    assert_eq!(audio.stats.generated_samples, 22050);
    assert_eq!(audio.stats.queued_samples, 22050);
    assert_eq!(audio.stats.overrun_samples, 0);
    assert_eq!(output.dropped, 0);
}

#[test]
fn busy_or_failed_output_retains_pcm_in_order() {
    let mut audio = NativeAudio::new(0.5);
    audio.render(&[(0.0, 1000.0, 0.5)], 0.0, 1.0 / 60.0, false);
    let first = audio.chunk().collect::<Vec<_>>();
    // Production commits only after queue_wave succeeds. No commit models a
    // busy wave, set_sample_count failure, buffer error or queue rejection.
    audio.render(&[], 1.0 / 60.0, 2.0 / 60.0, false);
    assert_eq!(audio.chunk().collect::<Vec<_>>(), first);
    assert_eq!(audio.pending_samples(), CHUNK_SAMPLES * 2);
    audio.commit_chunk();
    assert_eq!(audio.pending_samples(), CHUNK_SAMPLES);
    assert_eq!(audio.stats.queued_samples, CHUNK_SAMPLES as u64);
}

#[test]
fn future_events_survive_the_render_cap() {
    let events = [(0.0, 1000.0, 0.5), (0.08, 0.0, 0.5)];
    let mut audio = NativeAudio::new(0.5);
    let mut actual = Vec::new();
    audio.render(&events, 0.0, 0.1, false);
    drain(&mut audio, &mut actual);
    audio.render(&[], 0.1, 0.1, false);
    drain(&mut audio, &mut actual);
    audio.render(&[], 0.1, 0.1, false);
    drain(&mut audio, &mut actual);
    let mut expected = Vec::new();
    ToneSynth::new(SAMPLE_RATE, 0.5).render(&events, 0.0, 0.1, 4410, &mut expected);
    assert_eq!(actual, expected);
    assert_eq!(audio.stats.generated_samples, 4410);
}

#[test]
fn sub_sample_event_is_deferred_instead_of_applied_early() {
    let events = [(0.0, 1000.0, 0.5), (0.01666, 0.0, 0.5)];
    let mut audio = NativeAudio::new(0.5);
    let mut actual = Vec::new();
    audio.render(&events, 0.0, 0.01666, false); // 734 complete samples
    assert_eq!(audio.chunk_len(), 0);
    audio.render(&[], 0.01666, 2.0 / 60.0, false);
    drain(&mut audio, &mut actual);
    let mut expected = Vec::new();
    ToneSynth::new(SAMPLE_RATE, 0.5).render(&events, 0.0, 2.0 / 60.0, 1470, &mut expected);
    assert_eq!(actual, expected);
}

#[test]
fn partial_frame_power_cut_preserves_tone_then_silences() {
    let mut audio = NativeAudio::new(0.5);
    let mut actual = Vec::new();
    audio.render(&[(0.0, 1000.0, 0.5)], 0.0, 0.00832, true);
    audio.render(&[], 0.00832, 1.0 / 60.0, true);
    drain(&mut audio, &mut actual);
    let mut expected = Vec::new();
    ToneSynth::new(SAMPLE_RATE, 0.5).render(
        &[(0.0, 1000.0, 0.5), (0.00832, 0.0, 0.5)],
        0.0,
        1.0 / 60.0,
        735,
        &mut expected,
    );
    assert_eq!(actual, expected);
    assert!(actual[367..].iter().all(|&s| s == 0));
}

#[test]
fn wake_restarts_tone_at_the_core_event_time() {
    let events = [
        (0.0, 1000.0, 0.5),
        (0.00832, 0.0, 0.5),
        (0.020, 880.0, 0.25),
    ];
    let mut audio = NativeAudio::new(0.5);
    let mut actual = Vec::new();
    audio.render(&events[..1], 0.0, 0.00832, true);
    audio.render(&[], 0.00832, 1.0 / 60.0, true);
    drain(&mut audio, &mut actual);
    audio.render(&events[2..], 1.0 / 60.0, 2.0 / 60.0, false);
    drain(&mut audio, &mut actual);
    let mut expected = Vec::new();
    ToneSynth::new(SAMPLE_RATE, 0.5).render(&events, 0.0, 2.0 / 60.0, 1470, &mut expected);
    assert_eq!(actual, expected);
}

#[test]
fn stereo_refill_preserves_pcm_pointer_and_byte_length() {
    let mut stereo = Vec::with_capacity(CHUNK_SAMPLES);
    let pointer = stereo.as_ptr();
    for _ in 0..10 {
        stereo.resize(CHUNK_SAMPLES, [0, 0]);
        audio::interleave_mono((0..CHUNK_SAMPLES).map(|i| i as i16 - 300), &mut stereo);
        assert_eq!(stereo.as_ptr(), pointer);
        assert_eq!(stereo.len(), 735);
        assert_eq!(std::mem::size_of_val(stereo.as_slice()), 2940);
        for (i, pair) in stereo.iter().enumerate() {
            assert_eq!(*pair, [i as i16 - 300; 2]);
        }
    }
}

#[test]
fn prolonged_backend_failure_is_bounded_and_counted() {
    let mut audio = NativeAudio::new(0.5);
    for frame in 0..30 {
        audio.render(&[], frame as f64 / 60.0, (frame + 1) as f64 / 60.0, false);
    }
    assert_eq!(audio.pending_samples(), CHUNK_SAMPLES * 8);
    assert_eq!(
        audio.stats.overrun_samples,
        22050 - (CHUNK_SAMPLES * 8) as u64
    );
}

#[test]
fn silent_service_fallback_does_not_accumulate_pcm() {
    let mut audio = NativeAudio::new(0.5);
    for frame in 0..30 {
        audio.render(&[], frame as f64 / 60.0, (frame + 1) as f64 / 60.0, false);
        audio.discard_output();
    }
    assert_eq!(audio.pending_samples(), 0);
    assert_eq!(audio.stats.overrun_samples, 0);
}

#[test]
fn integer_sample_clock_has_no_long_term_rounding_loss() {
    let mut audio = NativeAudio::new(0.5);
    let mut count = 0;
    for frame in 0..3600 {
        audio.render(&[], frame as f64 / 60.0, (frame + 1) as f64 / 60.0, false);
        while audio.chunk_len() > 0 {
            count += audio.chunk_len();
            audio.commit_chunk();
        }
    }
    assert_eq!(count, SAMPLE_RATE as usize * 60);
    assert_eq!(audio.pending_samples(), 0);
}

#[test]
fn gameplay_priming_cushions_routine_frame_jitter() {
    let mut audio = NativeAudio::new(0.5);
    let mut output = legacy_audio::OutputModel::new();
    assert!(output.accept(0.0, 735));
    assert!(output.accept(0.0, 735));
    let mut wall = 0.0;
    for frame in 0..120 {
        wall += 1.0 / 60.0 + if frame % 2 == 0 { 0.003 } else { -0.003 };
        audio.render(&[], frame as f64 / 60.0, (frame + 1) as f64 / 60.0, false);
        while audio.chunk_len() > 0 {
            assert!(output.accept(wall, audio.chunk_len()));
            audio.commit_chunk();
        }
    }
    assert_eq!(output.dropped, 0);
    assert_eq!(output.underrun_seconds, 0.0);
    assert_eq!(audio.stats.overrun_samples, 0);
}
