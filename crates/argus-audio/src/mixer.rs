//! The mixer: voices routed to buses, buses to their parents, ducking
//! rules applied per block, equal-power panning, per-bus meters, and the
//! last seconds of the master kept for clip export (§8, §10). It owns no
//! device: the output callback pulls blocks from it in real time, and a
//! headless daemon advances it by sim time.

use crate::config::AudioConfig;
use crate::sound::Clip;

/// Frames mixed per block; ramps and ducking move once per block.
pub const BLOCK: usize = 256;
/// Seconds of master output kept for `audio.clip`.
pub const HISTORY_SECONDS: usize = 10;
/// The master limiter's ceiling: bursts are turned down to this, not clipped.
pub const CEILING: f32 = 0.95;
/// How long the limiter takes to let go, seconds.
pub const LIMIT_RELEASE: f32 = 0.15;

/// dB to linear.
pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Linear to dB, floored at -120.
pub fn lin_to_db(x: f32) -> f32 {
    if x <= 1e-6 {
        -120.0
    } else {
        20.0 * x.log10()
    }
}

/// One playing sound.
#[derive(Clone, Debug)]
pub struct Voice {
    /// Id, unique for the mixer's life.
    pub id: u64,
    clip: Clip,
    pos: f64,
    step: f64,
    gain: f32,
    /// Gain the voice is moving toward.
    pub target_gain: f32,
    pan: f32,
    target_pan: f32,
    /// Largest gain change per second (fades).
    ramp: f32,
    /// Bus index.
    pub bus: usize,
    looping: bool,
    stopping: bool,
    done: bool,
}

#[derive(Clone, Debug)]
struct Bus {
    name: String,
    parent: Option<usize>,
    gain: f32,
    mute: bool,
    duck: f32,
    peak: f32,
    sumsq: f64,
    n: u64,
}

#[derive(Clone, Debug)]
struct Duck {
    when: usize,
    duck: usize,
    lin: f32,
    factor: f32,
    attack_s: f32,
    release_s: f32,
}

/// A bus as `audio.status` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct BusMeter {
    /// Name.
    pub name: String,
    /// Parent.
    pub parent: Option<String>,
    /// Set gain, dB.
    pub gain_db: f32,
    /// Muted.
    pub mute: bool,
    /// Ducking currently applied, dB (0 when none).
    pub duck_db: f32,
    /// Peak since the last read, dBFS.
    pub peak_db: f32,
    /// RMS since the last read, dBFS.
    pub rms_db: f32,
}

/// The mixer.
pub struct Mixer {
    rate: u32,
    buses: Vec<Bus>,
    ducks: Vec<Duck>,
    voices: Vec<Voice>,
    next_id: u64,
    history: Vec<f32>,
    hist_pos: usize,
    hist_filled: usize,
    master_peak: f32,
    master_sumsq: f64,
    master_n: u64,
    rendered: u64,
    scratch: Vec<f32>,
    /// Per-bus accumulators, reused so the audio callback never allocates.
    acc: Vec<f32>,
    /// Per-bus subtree sums for the meters.
    tree: Vec<f32>,
    limiter: f32,
    limiter_min: f32,
}

impl Mixer {
    /// A mixer at `rate` with the buses and ducking rules of `cfg`.
    pub fn new(rate: u32, cfg: &AudioConfig) -> Mixer {
        let names: Vec<String> = cfg.buses.iter().map(|b| b.name.clone()).collect();
        let buses = cfg
            .buses
            .iter()
            .map(|b| Bus {
                name: b.name.clone(),
                parent: b
                    .parent
                    .as_ref()
                    .and_then(|p| names.iter().position(|n| n == p)),
                gain: db_to_lin(b.gain_db),
                mute: false,
                duck: 1.0,
                peak: 0.0,
                sumsq: 0.0,
                n: 0,
            })
            .collect();
        let ducks = cfg
            .ducks
            .iter()
            .filter_map(|d| {
                Some(Duck {
                    when: names.iter().position(|n| *n == d.when)?,
                    duck: names.iter().position(|n| *n == d.duck)?,
                    lin: db_to_lin(d.db),
                    factor: 1.0,
                    attack_s: d.attack_ms / 1000.0,
                    release_s: d.release_ms / 1000.0,
                })
            })
            .collect();
        Mixer {
            rate,
            buses,
            ducks,
            voices: Vec::new(),
            next_id: 1,
            history: vec![0.0; rate as usize * 2 * HISTORY_SECONDS],
            hist_pos: 0,
            hist_filled: 0,
            master_peak: 0.0,
            master_sumsq: 0.0,
            master_n: 0,
            rendered: 0,
            scratch: Vec::new(),
            acc: Vec::new(),
            tree: Vec::new(),
            limiter: 1.0,
            limiter_min: 1.0,
        }
    }

    /// Sample rate.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Frames rendered so far.
    pub fn rendered_frames(&self) -> u64 {
        self.rendered
    }

    /// Bus index by name.
    pub fn bus(&self, name: &str) -> Option<usize> {
        self.buses.iter().position(|b| b.name == name)
    }

    /// Bus names.
    pub fn bus_names(&self) -> Vec<String> {
        self.buses.iter().map(|b| b.name.clone()).collect()
    }

    /// Start a voice. `pitch` 1 is the clip's own speed; `pan` -1 left to 1 right.
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &mut self,
        clip: Clip,
        bus: usize,
        gain: f32,
        pan: f32,
        pitch: f32,
        looping: bool,
        fade_in_s: f32,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let (start, ramp) = if fade_in_s > 0.0 {
            (0.0, 1.0 / fade_in_s)
        } else {
            (gain, 200.0)
        };
        self.voices.push(Voice {
            id,
            clip,
            pos: 0.0,
            step: pitch.max(0.01) as f64,
            gain: start,
            target_gain: gain,
            pan: pan.clamp(-1.0, 1.0),
            target_pan: pan.clamp(-1.0, 1.0),
            ramp,
            bus: bus.min(self.buses.len() - 1),
            looping,
            stopping: false,
            done: false,
        });
        id
    }

    /// Move a voice's gain and pan (spatial updates, music layers).
    pub fn set(&mut self, id: u64, gain: f32, pan: f32) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
            if !v.stopping {
                v.target_gain = gain.max(0.0);
            }
            v.target_pan = pan.clamp(-1.0, 1.0);
        }
    }

    /// Set a voice's fade speed (full scale per `seconds`).
    pub fn set_ramp(&mut self, id: u64, seconds: f32) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
            v.ramp = 1.0 / seconds.max(0.001);
        }
    }

    /// Stop a voice, fading out over `fade_s` (0 is a 5 ms click-free stop).
    /// A voice that has not sounded yet is simply dropped, so a burst of
    /// requests in one tick never plays the voices it steals.
    pub fn stop(&mut self, id: u64, fade_s: f32) -> bool {
        if let Some(i) = self.voices.iter().position(|v| v.id == id && v.pos == 0.0) {
            self.voices.remove(i);
            return true;
        }
        match self.voices.iter_mut().find(|v| v.id == id) {
            Some(v) => {
                v.stopping = true;
                v.target_gain = 0.0;
                v.ramp = 1.0 / fade_s.max(0.005);
                true
            }
            None => false,
        }
    }

    /// Is the voice still playing?
    pub fn is_playing(&self, id: u64) -> bool {
        self.voices.iter().any(|v| v.id == id && !v.done)
    }

    /// Voices playing.
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Voices playing per bus.
    pub fn voices_on(&self, bus: usize) -> usize {
        self.voices.iter().filter(|v| v.bus == bus).count()
    }

    /// Set a bus's gain and mute (tuning).
    pub fn set_bus(&mut self, bus: usize, gain_db: Option<f32>, mute: Option<bool>) {
        if let Some(b) = self.buses.get_mut(bus) {
            if let Some(g) = gain_db {
                b.gain = db_to_lin(g);
            }
            if let Some(m) = mute {
                b.mute = m;
            }
        }
    }

    fn under(&self, mut bus: usize, ancestor: usize) -> bool {
        loop {
            if bus == ancestor {
                return true;
            }
            match self.buses[bus].parent {
                Some(p) => bus = p,
                None => return false,
            }
        }
    }

    fn effective(&self) -> Vec<f32> {
        (0..self.buses.len())
            .map(|mut i| {
                let mut g = 1.0;
                loop {
                    let b = &self.buses[i];
                    g *= if b.mute { 0.0 } else { b.gain * b.duck };
                    match b.parent {
                        Some(p) => i = p,
                        None => break g,
                    }
                }
            })
            .collect()
    }

    /// Mix into `out`, interleaved stereo; `out.len() / 2` frames.
    pub fn render(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(BLOCK);
            self.block(&mut out[done * 2..(done + n) * 2], n);
            done += n;
        }
    }

    /// Advance `frames` without keeping the output (headless catch-up):
    /// voices move and end, the history and meters are fed.
    pub fn advance(&mut self, frames: usize) {
        let mut buf = std::mem::take(&mut self.scratch);
        buf.resize(BLOCK * 2, 0.0);
        let mut left = frames;
        while left > 0 {
            let n = left.min(BLOCK);
            self.block(&mut buf[..n * 2], n);
            left -= n;
        }
        self.scratch = buf;
    }

    fn block(&mut self, out: &mut [f32], n: usize) {
        let secs = n as f32 / self.rate as f32;
        // Ducking: a rule is active while any voice under `when` is audible.
        for i in 0..self.ducks.len() {
            let (when, lin) = (self.ducks[i].when, self.ducks[i].lin);
            let active = self
                .voices
                .iter()
                .any(|v| !v.done && v.target_gain > 1e-3 && self.under(v.bus, when));
            let d = &mut self.ducks[i];
            let target = if active { lin } else { 1.0 };
            let span = (1.0 - d.lin).max(1e-6);
            if d.factor > target {
                d.factor = (d.factor - span * secs / d.attack_s).max(target);
            } else {
                d.factor = (d.factor + span * secs / d.release_s).min(target);
            }
        }
        for b in &mut self.buses {
            b.duck = 1.0;
        }
        for d in &self.ducks {
            self.buses[d.duck].duck *= d.factor;
        }
        if self.voices.iter().all(|v| v.done) {
            self.silent_block(out, n, secs);
            return;
        }
        let eff = self.effective();
        let nb = self.buses.len();
        let mut acc = std::mem::take(&mut self.acc);
        acc.clear();
        acc.resize(nb * n * 2, 0.0);
        for v in &mut self.voices {
            if v.done {
                continue;
            }
            let g0 = v.gain;
            let max_step = v.ramp * secs;
            let g1 = if v.target_gain > g0 {
                (g0 + max_step).min(v.target_gain)
            } else {
                (g0 - max_step).max(v.target_gain)
            };
            let (p0, p1) = (v.pan, v.target_pan);
            let base = v.bus * n * 2;
            let len = v.clip.len() as f64;
            for i in 0..n {
                if v.pos >= len {
                    if v.looping && len > 0.0 {
                        v.pos -= len;
                    } else {
                        v.done = true;
                        break;
                    }
                }
                let j = v.pos as usize;
                let t = (v.pos - j as f64) as f32;
                let a = v.clip[j];
                let b = if j + 1 < v.clip.len() {
                    v.clip[j + 1]
                } else if v.looping {
                    v.clip[0]
                } else {
                    0.0
                };
                let s = a + (b - a) * t;
                let k = i as f32 / n as f32;
                let g = g0 + (g1 - g0) * k;
                let pan = p0 + (p1 - p0) * k;
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                acc[base + i * 2] += s * g * angle.cos();
                acc[base + i * 2 + 1] += s * g * angle.sin();
                v.pos += v.step;
            }
            v.gain = g1;
            v.pan = p1;
            if v.stopping && v.gain <= 0.0 {
                v.done = true;
            }
        }
        self.voices.retain(|v| !v.done);
        out.fill(0.0);
        // A bus's meter is everything it sends on: its own voices plus its
        // children's, so the master row is the whole mix before the limiter.
        let mut tree = std::mem::take(&mut self.tree);
        tree.clear();
        tree.resize(nb * n * 2, 0.0);
        for (bi, &g) in eff.iter().enumerate() {
            if g == 0.0 {
                continue;
            }
            let base = bi * n * 2;
            let mut chain = Some(bi);
            let mut first = true;
            while let Some(a) = chain {
                let tb = a * n * 2;
                for i in 0..n * 2 {
                    let s = acc[base + i] * g;
                    tree[tb + i] += s;
                    if first {
                        out[i] += s;
                    }
                }
                first = false;
                chain = self.buses[a].parent;
            }
        }
        for (bi, b) in self.buses.iter_mut().enumerate() {
            let tb = bi * n * 2;
            for i in 0..n * 2 {
                let s = tree[tb + i];
                let a = s.abs();
                if a > b.peak {
                    b.peak = a;
                }
                b.sumsq += (s * s) as f64;
            }
            b.n += (n * 2) as u64;
        }
        self.tree = tree;
        // The master limiter: a burst of voices is turned down, not clipped.
        let block_peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let want = if block_peak > CEILING {
            CEILING / block_peak
        } else {
            1.0
        };
        let l0 = self.limiter;
        let l1 = if want < l0 {
            want
        } else {
            (l0 + secs / LIMIT_RELEASE).min(want)
        };
        self.limiter = l1;
        self.limiter_min = self.limiter_min.min(l1).min(l0);
        if l0 < 1.0 || l1 < 1.0 {
            for (i, s) in out.iter_mut().enumerate() {
                let k = (i / 2) as f32 / n as f32;
                // Attack is immediate: a peak is never let through at the old gain.
                let g = if l1 < l0 { l1 } else { l0 + (l1 - l0) * k };
                *s *= g;
            }
        }
        let hl = self.history.len();
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
            let a = s.abs();
            if a > self.master_peak {
                self.master_peak = a;
            }
            self.master_sumsq += (*s * *s) as f64;
            self.history[self.hist_pos] = *s;
            self.hist_pos = (self.hist_pos + 1) % hl;
        }
        self.master_n += (n * 2) as u64;
        self.acc = acc;
        self.hist_filled = (self.hist_filled + n * 2).min(hl);
        self.rendered += n as u64;
    }

    /// `block` with no voice playing, leaving exactly the state it would:
    /// meters count the samples, the limiter recovers, the history gets
    /// zeros. Mixing zeros through every bus cost a discrete game a second of
    /// DSP per turn with no device open, which made Delve's tests five times
    /// slower (found after the phase-1 gate).
    fn silent_block(&mut self, out: &mut [f32], n: usize, secs: f32) {
        self.voices.clear();
        out.fill(0.0);
        for b in &mut self.buses {
            b.n += (n * 2) as u64;
        }
        let l0 = self.limiter;
        let l1 = (l0 + secs / LIMIT_RELEASE).min(1.0);
        self.limiter = l1;
        self.limiter_min = self.limiter_min.min(l1).min(l0);
        let hl = self.history.len();
        let mut left = (n * 2).min(hl);
        while left > 0 {
            let run = left.min(hl - self.hist_pos);
            self.history[self.hist_pos..self.hist_pos + run].fill(0.0);
            self.hist_pos = (self.hist_pos + run) % hl;
            left -= run;
        }
        if n * 2 > hl {
            // More than the ring holds: the write position still moves on by
            // every sample, as the slow path's would.
            self.hist_pos = (self.hist_pos + (n * 2 - hl)) % hl;
        }
        self.master_n += (n * 2) as u64;
        self.hist_filled = (self.hist_filled + n * 2).min(hl);
        self.rendered += n as u64;
    }

    /// The most the limiter turned the master down since the last call,
    /// dB (0 when it never acted), then reset.
    pub fn take_limiter_db(&mut self) -> f32 {
        let db = if self.limiter_min < 0.9999 {
            lin_to_db(self.limiter_min)
        } else {
            0.0
        };
        self.limiter_min = self.limiter;
        db
    }

    /// Meters for every bus since the last call, then reset.
    pub fn take_meters(&mut self) -> (Vec<BusMeter>, f32, f32) {
        let names: Vec<String> = self.buses.iter().map(|b| b.name.clone()).collect();
        let out = self
            .buses
            .iter_mut()
            .map(|b| {
                let m = BusMeter {
                    name: b.name.clone(),
                    parent: b.parent.map(|p| names[p].clone()),
                    gain_db: lin_to_db(b.gain),
                    mute: b.mute,
                    duck_db: if b.duck < 0.9999 {
                        lin_to_db(b.duck)
                    } else {
                        0.0
                    },
                    peak_db: lin_to_db(b.peak),
                    rms_db: if b.n == 0 {
                        -120.0
                    } else {
                        lin_to_db(((b.sumsq / b.n as f64) as f32).sqrt())
                    },
                };
                b.peak = 0.0;
                b.sumsq = 0.0;
                b.n = 0;
                m
            })
            .collect();
        let peak = lin_to_db(self.master_peak);
        let rms = if self.master_n == 0 {
            -120.0
        } else {
            lin_to_db(((self.master_sumsq / self.master_n as f64) as f32).sqrt())
        };
        self.master_peak = 0.0;
        self.master_sumsq = 0.0;
        self.master_n = 0;
        (out, peak, rms)
    }

    /// The last `seconds` of master output, interleaved stereo, oldest first.
    pub fn history(&self, seconds: f32) -> Vec<f32> {
        let want = ((seconds.max(0.0) * self.rate as f32) as usize * 2).min(self.hist_filled);
        let hl = self.history.len();
        let start = (self.hist_pos + hl - want) % hl;
        (0..want).map(|i| self.history[(start + i) % hl]).collect()
    }
}
