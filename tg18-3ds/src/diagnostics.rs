//! Low-overhead, continuously sampled performance diagnostics.
//! Uses a 10-second ring of 1-second buckets. No allocations in record().
use super::blend_rgba_bottom;
use std::time::Instant;

const PX: usize = 10;
const PY: usize = 27;
const PW: usize = 300;
const PH: usize = 206;
const PANEL: &[u8; PW * PH * 4] = include_bytes!("../assets/runtime/diagnostics_panel.rgba");
const IDLE_PANEL: &[u8; PW * PH * 4] =
    include_bytes!("../assets/runtime/diagnostics_idle_panel.rgba");
const GW: usize = 8;
const GH: usize = 13;
const CHARS: &[u8] = b"0123456789. /MSFPNO";
const GLYPHS: &[u8; GW * GH * 4 * CHARS.len()] =
    include_bytes!("../assets/runtime/diagnostics_glyphs.rgba");
const ROWS: [usize; 11] = [60, 72, 84, 96, 108, 120, 132, 144, 156, 168, 180];
const WINDOW_SECS: u64 = 10;
const SPIKE_US: u64 = 100_000;

#[derive(Clone, Copy, Default)]
pub struct Snapshot {
    pub emu_avg: u32,
    pub emu_min: u32,
    pub emu_max: u32,
    pub audio_avg: u32,
    pub top_avg: u32,
    pub bottom_avg: u32,
    pub work_avg: u32,
    pub frame_avg: u32,
    pub fps: u32,
    pub spikes: u32,
    pub last_spike_us: u32,
    pub last_spike_age: u32,
    /// Rolling-window idle-probe stats: attempted, no-repeat, memory-changed,
    /// MMIO-written, passes proven safe. These are core counters, not guesses.
    pub probe_stats: [u64; 5],
    pub idle_skipped: u64,
}

#[derive(Clone, Copy, Default)]
struct Bucket {
    sums: [u64; 7],
    frames: u32,
    spikes: u32,
    emu_min: u64,
    emu_max: u64,
    probe_stats: [u64; 5],
    idle_skipped: u64,
}

pub struct Profiler {
    started: Instant,
    last_second: u64,
    buckets: [Bucket; WINDOW_SECS as usize],
    last_spike: Option<(u64, Instant)>,
    snapshot: Snapshot,
    refresh: bool,
}

impl Profiler {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            last_second: 0,
            buckets: [Bucket::default(); WINDOW_SECS as usize],
            last_spike: None,
            snapshot: Snapshot::default(),
            refresh: true,
        }
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn take_refresh(&mut self) -> bool {
        let should_refresh = self.refresh;
        self.refresh = false;
        should_refresh
    }

    // samples: [emulation, audio, top draw, bottom draw, CPU work,
    //           vsync wait, total loop] in microseconds.
    // Called every loop, including with the diagnostics panel CLOSED.
    pub fn record(&mut self, samples: [u64; 7], probe_delta: [u64; 5], skipped_delta: u64) {
        let second = self.started.elapsed().as_secs();
        if second != self.last_second {
            // Finalize the full preceding interval BEFORE expiring its oldest
            // bucket, otherwise the 10-second FPS average loses one second.
            self.update_snapshot();
            self.refresh = true;
            let elapsed = second.saturating_sub(self.last_second);
            if elapsed >= WINDOW_SECS {
                self.buckets = [Bucket::default(); WINDOW_SECS as usize];
            } else {
                for step in 1..=elapsed {
                    let index = ((self.last_second + step) % WINDOW_SECS) as usize;
                    self.buckets[index] = Bucket::default();
                }
            }
            self.last_second = second;
        }
        let bucket = &mut self.buckets[(second % WINDOW_SECS) as usize];
        if bucket.frames == 0 {
            bucket.emu_min = samples[0];
        }
        bucket.emu_min = bucket.emu_min.min(samples[0]);
        bucket.emu_max = bucket.emu_max.max(samples[0]);
        for (total, current) in bucket.sums.iter_mut().zip(samples.iter()) {
            *total = total.saturating_add(*current);
        }
        bucket.frames = bucket.frames.saturating_add(1);
        for (sum, delta) in bucket.probe_stats.iter_mut().zip(probe_delta.iter()) {
            *sum = sum.saturating_add(*delta);
        }
        bucket.idle_skipped = bucket.idle_skipped.saturating_add(skipped_delta);
        if samples[0] >= SPIKE_US {
            bucket.spikes = bucket.spikes.saturating_add(1);
            self.last_spike = Some((samples[0], Instant::now()));
        }
    }

    fn update_snapshot(&mut self) {
        let mut totals = [0u64; 7];
        let mut n = 0u64;
        let mut spikes = 0u64;
        let mut low = u64::MAX;
        let mut high = 0u64;
        let mut probe_stats = [0u64; 5];
        let mut idle_skipped = 0u64;
        for b in &self.buckets {
            if b.frames == 0 {
                continue;
            }
            n += b.frames as u64;
            spikes += b.spikes as u64;
            low = low.min(b.emu_min);
            high = high.max(b.emu_max);
            for (sum, part) in probe_stats.iter_mut().zip(b.probe_stats.iter()) {
                *sum = sum.saturating_add(*part);
            }
            idle_skipped = idle_skipped.saturating_add(b.idle_skipped);
            for (sum, part) in totals.iter_mut().zip(b.sums.iter()) {
                *sum = sum.saturating_add(*part);
            }
        }
        let avg = |i: usize| (totals[i] / n.max(1)).min(u32::MAX as u64) as u32;
        // Before the first 10 seconds, divide by actual elapsed time.
        let elapsed_ms = self.started.elapsed().as_millis().min(10_000).max(1) as u64;
        let (spike_us, age) = match self.last_spike {
            Some((us, when)) => (
                us.min(u32::MAX as u64) as u32,
                when.elapsed().as_secs().min(u32::MAX as u64) as u32,
            ),
            None => (0, 0),
        };
        self.snapshot = Snapshot {
            emu_avg: avg(0),
            emu_min: if n == 0 {
                0
            } else {
                low.min(u32::MAX as u64) as u32
            },
            emu_max: high.min(u32::MAX as u64) as u32,
            audio_avg: avg(1),
            top_avg: avg(2),
            bottom_avg: avg(3),
            work_avg: avg(4),
            frame_avg: avg(6),
            fps: (n.saturating_mul(1000) / elapsed_ms).min(u32::MAX as u64) as u32,
            spikes: spikes.min(u32::MAX as u64) as u32,
            last_spike_us: spike_us,
            last_spike_age: age,
            probe_stats,
            idle_skipped,
        };
    }
}

fn text_right(buf: &mut [u8], row: usize, content: &str) {
    let len = content.len();
    let mut x = PX + 277usize.saturating_sub(len * GW);
    let y = PY + ROWS[row];
    let stride = GW * GH * 4;
    for &byte in content.as_bytes() {
        if let Some(index) = CHARS.iter().position(|&b| b == byte) {
            blend_rgba_bottom(
                buf,
                x,
                y,
                GW,
                GH,
                &GLYPHS[index * stride..(index + 1) * stride],
            );
        }
        x += GW;
    }
}

pub fn draw(buf: &mut [u8], snap: &Snapshot, idle_page: bool) {
    blend_rgba_bottom(
        buf,
        PX,
        PY,
        PW,
        PH,
        if idle_page { IDLE_PANEL } else { PANEL },
    );
    if idle_page {
        // A proven-safe pass is not necessarily one whole successful skip;
        // the core can record multiple passes per probe. Show raw counters.
        for (i, stat) in snap.probe_stats.iter().enumerate() {
            text_right(buf, i, &stat.to_string());
        }
        text_right(
            buf,
            5,
            &format!("{:.1} M", snap.idle_skipped as f64 / 1_000_000.0),
        );
        text_right(buf, 6, &format!("{:.1} MS", snap.emu_avg as f32 / 1000.0));
        text_right(buf, 7, &format!("{:.1} MS", snap.emu_max as f32 / 1000.0));
        text_right(buf, 8, &snap.spikes.to_string());
        text_right(buf, 9, &snap.fps.to_string());
        return;
    }
    for (i, us) in [
        snap.emu_avg,
        snap.emu_min,
        snap.emu_max,
        snap.audio_avg,
        snap.top_avg,
        snap.bottom_avg,
        snap.work_avg,
        snap.frame_avg,
    ]
    .iter()
    .enumerate()
    {
        text_right(buf, i, &format!("{:.1} MS", *us as f32 / 1000.0));
    }
    text_right(buf, 8, &format!("{}", snap.fps));
    text_right(buf, 9, &format!("{}", snap.spikes));
    text_right(
        buf,
        10,
        &format!(
            "{:.1} MS / {} S",
            snap.last_spike_us as f32 / 1000.0,
            snap.last_spike_age
        ),
    );
}

pub fn close_hit(x: i32, y: i32) -> bool {
    x >= (PX + 261) as i32 && x < (PX + 292) as i32 && y >= (PY + 6) as i32 && y < (PY + 38) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn aggregates_ten_seconds_and_expires_the_oldest_bucket() {
        let mut profiler = Profiler::new();
        for second in 0..10 {
            profiler.started = Instant::now() - Duration::from_secs(second);
            for _ in 0..60 {
                profiler.record(
                    [1_000 + second, 200, 300, 400, 1_900, 14_767, 16_667],
                    [1, 2, 3, 4, 5],
                    10,
                );
            }
        }
        profiler.started = Instant::now() - Duration::from_secs(10);
        profiler.record([2_000, 200, 300, 400, 2_900, 13_767, 16_667], [0; 5], 0);
        let snap = profiler.snapshot();
        assert_eq!(snap.emu_min, 1_000);
        assert_eq!(snap.emu_max, 1_009);
        assert_eq!(snap.probe_stats, [600, 1200, 1800, 2400, 3000]);
        assert_eq!(snap.idle_skipped, 6_000);
        assert_eq!(snap.fps, 60);
        // Bucket 0 was expired after finalizing the preceding ten seconds.
        assert_eq!(profiler.buckets[0].frames, 1);
        assert_eq!(profiler.buckets[0].probe_stats, [0; 5]);
    }

    #[test]
    fn detects_spikes_without_diagnostics_being_open() {
        let mut profiler = Profiler::new();
        profiler.record([100_000, 0, 0, 0, 100_000, 0, 100_000], [0; 5], 0);
        profiler.started = Instant::now() - Duration::from_secs(1);
        profiler.record([1_000, 0, 0, 0, 1_000, 0, 1_000], [0; 5], 0);
        assert_eq!(profiler.snapshot().spikes, 1);
        assert_eq!(profiler.snapshot().last_spike_us, 100_000);
    }
}
