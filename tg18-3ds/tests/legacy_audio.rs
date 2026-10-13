//! Frozen scheduling formula from the uploaded v49 and revised frontends.
//! The two versions generate the same PCM; their NDSP playback lengths differ.
use tg18::sound::ToneSynth;

pub struct LegacyAudio {
    synth: ToneSynth,
    cursor: f64,
    stretch: f64,
}

impl LegacyAudio {
    pub fn new() -> Self {
        Self {
            synth: ToneSynth::new(44_100, 0.5),
            cursor: 0.0,
            stretch: 1.0,
        }
    }

    pub fn render(
        &mut self,
        now: f64,
        real_dt: f64,
        events: &[(f64, f64, f64)],
    ) -> (usize, Vec<i16>) {
        let rate = 44_100.0;
        let n_emu = (((now - self.cursor) * rate).max(0.0) as usize).min(2048);
        let end_t = self.cursor + n_emu as f64 / rate;
        let raw = real_dt / (n_emu as f64 / rate).max(0.001);
        if raw > self.stretch {
            self.stretch = raw;
        } else {
            self.stretch = self.stretch * 0.8 + raw * 0.2;
        }
        if self.stretch < 1.25 {
            self.stretch = 1.0;
        }
        let n = ((n_emu as f64 * self.stretch) as usize).clamp(n_emu, 24000);
        let mut out = Vec::with_capacity(n);
        self.synth.render(events, self.cursor, end_t, n, &mut out);
        self.cursor = end_t;
        (n_emu, out)
    }
}

/// FIFO-wave playback model. Queued chunks play sequentially at 44,100 Hz.
/// It models durations/status occupancy, not DSP cache coherency or speakers.
pub struct OutputModel {
    finishes: [f64; 4],
    next: usize,
    pub dropped: usize,
    pub samples: usize,
    pub underrun_seconds: f64,
    started: bool,
}

impl OutputModel {
    pub fn new() -> Self {
        Self {
            finishes: [0.0; 4],
            next: 0,
            dropped: 0,
            samples: 0,
            underrun_seconds: 0.0,
            started: false,
        }
    }
    pub fn accept(&mut self, wall: f64, samples: usize) -> bool {
        if self.finishes[self.next] > wall {
            self.dropped += 1;
            return false;
        }
        let last_finish = self.finishes.iter().copied().fold(0.0, f64::max);
        if self.started {
            self.underrun_seconds += (wall - last_finish).max(0.0);
        }
        self.started = true;
        let tail = last_finish.max(wall);
        self.finishes[self.next] = tail + samples as f64 / 44_100.0;
        self.next = (self.next + 1) % self.finishes.len();
        self.samples += samples;
        true
    }
}
