//! Native-rate buzzer rendering and a bounded PCM FIFO, independent of NDSP.
//!
//! Wall-clock stalls must not lengthen later notes. Render sample positions
//! from emulated time, and keep PCM until the output backend accepts it.
use std::collections::VecDeque;
use tg18::sound::ToneSynth;

pub const SAMPLE_RATE: u32 = 44_100;
pub const CHUNK_SAMPLES: usize = SAMPLE_RATE as usize / 60;
const RENDER_CAP: usize = 2_048;
const FIFO_CAP: usize = CHUNK_SAMPLES * 8;
type Event = (f64, f64, f64);

#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    pub generated_samples: u64,
    pub queued_samples: u64,
    pub overrun_samples: u64,
    pub buffer_errors: u64,
    pub sample_count_errors: u64,
    pub cache_flush_errors: u64,
    pub queue_errors: u64,
}

pub struct NativeAudio {
    synth: ToneSynth,
    cursor: Option<u64>,
    events: Vec<Event>,
    samples: Vec<i16>,
    pending: VecDeque<i16>,
    powered_off: bool,
    pub stats: Stats,
}

fn sample_at(time: f64) -> u64 {
    // Snap only numerical noise at exact sample boundaries. The tolerance is
    // 1e-7 of a sample, not a timing or speed adjustment.
    (time.max(0.0) * SAMPLE_RATE as f64 + 1e-7).floor() as u64
}

impl NativeAudio {
    pub fn new(volume: f64) -> Self {
        Self {
            synth: ToneSynth::new(SAMPLE_RATE, volume),
            cursor: None,
            events: Vec::with_capacity(32),
            samples: Vec::with_capacity(RENDER_CAP),
            pending: VecDeque::with_capacity(FIFO_CAP),
            powered_off: false,
            stats: Stats::default(),
        }
    }

    /// Accept each batch of core events once. Future events remain pending if
    /// rounding or the per-loop work cap leaves them beyond this render span.
    pub fn render(&mut self, events: &[Event], before: f64, now: f64, powered_off: bool) {
        self.events.extend_from_slice(events);
        if powered_off && !self.powered_off {
            // Preserve any tone before the power cut, then silence the buzzer
            // at the cut's actual emulated time (including a partial frame).
            self.events.push((now, 0.0, 0.5));
        }
        self.powered_off = powered_off;
        let start = *self.cursor.get_or_insert_with(|| sample_at(before));
        let target = sample_at(now);
        let count = target.saturating_sub(start).min(RENDER_CAP as u64) as usize;
        let end = start + count as u64;
        let t0 = start as f64 / SAMPLE_RATE as f64;
        let t1 = end as f64 / SAMPLE_RATE as f64;
        let consumed = self.events.partition_point(|event| event.0 <= t1);
        self.samples.clear();
        self.synth
            .render(&self.events[..consumed], t0, t1, count, &mut self.samples);
        self.events.drain(..consumed);
        self.cursor = Some(end);
        debug_assert_eq!(self.samples.len(), count);
        self.stats.generated_samples += count as u64;

        // A failing/stalled backend cannot grow memory or audio latency without
        // bound. Keep the freshest samples and report every overrun explicitly.
        let overflow = (self.pending.len() + count).saturating_sub(FIFO_CAP);
        if overflow > 0 {
            self.pending.drain(..overflow);
            self.stats.overrun_samples += overflow as u64;
        }
        self.pending.extend(self.samples.iter().copied());
    }

    pub fn chunk_len(&self) -> usize {
        if self.pending.len() >= CHUNK_SAMPLES {
            CHUNK_SAMPLES
        } else {
            0
        }
    }

    pub fn chunk(&self) -> impl Iterator<Item = i16> + '_ {
        self.pending.iter().copied().take(self.chunk_len())
    }

    /// Called only after successful NDSP queueing, never for a busy/error wave.
    pub fn commit_chunk(&mut self) {
        let count = self.chunk_len();
        self.pending.drain(..count);
        self.stats.queued_samples += count as u64;
    }

    /// Audio service unavailable: keep the event clock current, without a FIFO
    /// backlog in the intentionally silent fallback.
    pub fn discard_output(&mut self) {
        self.pending.clear();
    }

    pub fn pending_samples(&self) -> usize {
        self.pending.len()
    }
}

pub fn interleave_mono(samples: impl Iterator<Item = i16>, stereo: &mut [[i16; 2]]) {
    for (dst, sample) in stereo.iter_mut().zip(samples) {
        *dst = [sample, sample];
    }
}
