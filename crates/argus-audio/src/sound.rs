//! Sample data: sound files (WAV, Ogg Vorbis) decoded to mono at the mixer
//! rate, and described tones synthesised the same way, so every variation
//! is one `Clip`.

use crate::config::{Tone, Wave};
use argus_ecs::{ArgusError, Result};
use std::path::Path;
use std::sync::Arc;

/// Mono samples at the mixer rate.
pub type Clip = Arc<[f32]>;

/// A sound file's samples as authored.
pub struct Decoded {
    /// Sample rate.
    pub rate: u32,
    /// Channels, interleaved in `samples`.
    pub channels: u16,
    /// Interleaved samples in [-1, 1].
    pub samples: Vec<f32>,
}

impl Decoded {
    /// Frames (samples per channel).
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    /// The loudest sample, linear.
    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0, |m, s| m.max(s.abs()))
    }
}

/// Decode a sound file's bytes: a WAV (8/16/24/32-bit integer or 32-bit
/// float) or Ogg Vorbis, told apart by their first bytes. A release pack
/// keeps a sound's authored path and holds it as Vorbis (phase 5 item 1),
/// so a name says nothing about the format.
pub fn decode(bytes: &[u8]) -> std::result::Result<Decoded, String> {
    if bytes.starts_with(b"RIFF") {
        let mut r =
            hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|x| x.to_string())?;
        let spec = r.spec();
        let samples = match spec.sample_format {
            hound::SampleFormat::Float => r
                .samples::<f32>()
                .collect::<std::result::Result<_, _>>()
                .map_err(|x| x.to_string())?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
                r.samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<std::result::Result<_, _>>()
                    .map_err(|x| x.to_string())?
            }
        };
        Ok(Decoded {
            rate: spec.sample_rate,
            channels: spec.channels.max(1),
            samples,
        })
    } else if bytes.starts_with(b"OggS") {
        let mut r = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes))
            .map_err(|x| format!("not Ogg Vorbis: {x}"))?;
        let mut samples = Vec::new();
        while let Some(p) = r.read_dec_packet_itl().map_err(|x| x.to_string())? {
            samples.extend(p.iter().map(|&s| s as f32 / 32768.0));
        }
        Ok(Decoded {
            rate: r.ident_hdr.audio_sample_rate,
            channels: r.ident_hdr.audio_channels.max(1) as u16,
            samples,
        })
    } else {
        Err("neither a WAV nor Ogg Vorbis".to_string())
    }
}

/// Decode a sound file (see [`decode`]) to mono at `rate`. It is read
/// through the engine's file system, so a release build's sounds, which
/// are in memory, load too.
pub fn load(path: &Path, rate: u32) -> Result<Clip> {
    let e = |m: String| {
        ArgusError::new(crate::codes::BAD_SOUND, format!("{}: {m}", path.display()))
            .hint("Sounds are WAV or Ogg Vorbis files under assets/; a described tone in audio.toml works as a placeholder.")
    };
    let bytes = argus_os::fs::read(path).map_err(|x| e(x.to_string()))?;
    let d = decode(&bytes).map_err(e)?;
    let ch = d.channels as usize;
    let mono: Vec<f32> = d
        .samples
        .chunks(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok(resample(&mono, d.rate, rate).into())
}

/// Linear resampling; good enough for effects, and music is authored at
/// the mixer rate in practice.
pub fn resample(src: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || src.is_empty() {
        return src.to_vec();
    }
    let n = ((src.len() as u64 * to as u64) / from as u64).max(1) as usize;
    let step = from as f64 / to as f64;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let j = p as usize;
            let t = (p - j as f64) as f32;
            let a = src[j.min(src.len() - 1)];
            let b = src[(j + 1).min(src.len() - 1)];
            a + (b - a) * t
        })
        .collect()
}

/// [`synth`] for many tones, in their order: at once on as many threads as
/// the machine has, where it has threads. A game's music is seconds of
/// samples a layer, and making Hollow's one after another was 50 ms of
/// every open, a sixth of section 17's budget (found at the phase 3 gate).
pub fn synth_all(tones: &[&Tone], rate: u32) -> Vec<Clip> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(tones.len());
        if threads > 1 {
            // The longest first, each to the thread with the least to make.
            let len = |t: &Tone| {
                let n = if t.notes.is_empty() {
                    1.0
                } else {
                    t.notes.len() as f32
                };
                n * if t.notes.is_empty() { t.ms } else { t.note_ms }
            };
            let mut order: Vec<usize> = (0..tones.len()).collect();
            order.sort_by(|a, b| len(tones[*b]).total_cmp(&len(tones[*a])));
            let mut load = vec![0.0f32; threads];
            let mut work: Vec<Vec<usize>> = vec![Vec::new(); threads];
            for i in order {
                let (w, _) = load
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.total_cmp(b.1))
                    .expect("a thread");
                load[w] += len(tones[i]);
                work[w].push(i);
            }
            let mut out: Vec<Option<Clip>> = vec![None; tones.len()];
            std::thread::scope(|s| {
                let handles: Vec<_> = work
                    .iter()
                    .map(|mine| {
                        s.spawn(move || {
                            mine.iter()
                                .map(|&i| (i, synth(tones[i], rate)))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                for h in handles {
                    for (i, c) in h.join().expect("a tone") {
                        out[i] = Some(c);
                    }
                }
            });
            return out.into_iter().map(|c| c.expect("made")).collect();
        }
    }
    tones.iter().map(|t| synth(t, rate)).collect()
}

/// Synthesise a described tone at `rate`. Deterministic: noise uses a
/// fixed seed, and the sines are `libm`'s (pure Rust, not the platform's),
/// so the same description is the same samples everywhere, the browser
/// included.
pub fn synth(t: &Tone, rate: u32) -> Clip {
    let sr = rate as f32;
    let notes: Vec<f32> = if t.notes.is_empty() {
        vec![t.freq]
    } else {
        t.notes.clone()
    };
    let note_len = if t.notes.is_empty() {
        (t.ms / 1000.0 * sr) as usize
    } else {
        (t.note_ms / 1000.0 * sr) as usize
    }
    .max(1);
    let attack = (t.attack_ms / 1000.0 * sr) as usize;
    let release = (t.release_ms / 1000.0 * sr) as usize;
    let mut out = Vec::with_capacity(note_len * notes.len());
    let mut noise: u32 = 0x9E37_79B9;
    let mut held = 0.0f32;
    let mut phase = 0.0f32;
    let tau = std::f32::consts::TAU;
    let repeat = t.repeat_ms / 1000.0;
    // The filters' and the phaser's state carries on from note to note.
    let (mut low, mut band) = (0.0f32, 0.0f32);
    let (mut hp_in, mut hp_out) = (0.0f32, 0.0f32);
    let mut delay = vec![0.0f32; if t.phaser_ms > 0.0 { 4096 } else { 0 }];
    let mut at = 0usize;
    let body = note_len.saturating_sub(attack + release).max(1);
    for &f0 in &notes {
        for i in 0..note_len {
            let secs = i as f32 / sr;
            // The pitch's own clock: it starts over every repeat.
            let pt = if repeat > 0.0 { secs % repeat } else { secs };
            let mut f = f0 + t.slide * pt + 0.5 * t.slide_accel * pt * pt;
            if t.jump != 1.0 && pt * 1000.0 >= t.jump_ms {
                f *= t.jump;
            }
            if t.vibrato != 0.0 {
                f *= 1.0 + t.vibrato * libm::sinf(tau * t.vibrato_hz * secs);
            }
            let f = f.max(0.0);
            let before = phase;
            phase = (phase + f / sr).fract();
            let mut s = match t.wave {
                Wave::Sine => libm::sinf(phase * tau),
                Wave::Square => {
                    let duty = if t.duty_sweep != 0.0 {
                        (t.duty + t.duty_sweep * secs).clamp(0.02, 0.98)
                    } else {
                        t.duty
                    };
                    if phase < duty {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Saw => 2.0 * phase - 1.0,
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Noise => {
                    // Pitched, a new value each half period; else each sample.
                    let fresh = !t.noise_pitch
                        || phase < before
                        || (before < 0.5 && phase >= 0.5)
                        || out.is_empty();
                    if fresh {
                        noise ^= noise << 13;
                        noise ^= noise >> 17;
                        noise ^= noise << 5;
                        held = (noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
                    }
                    held
                }
            };
            if t.lowpass > 0.0 {
                // A two-pole state-variable filter.
                let fc = (t.lowpass + t.lowpass_sweep * secs).clamp(20.0, sr * 0.2);
                let k = 2.0 * libm::sinf(std::f32::consts::PI * fc / sr);
                let damp = 1.4 * (1.0 - 0.9 * t.resonance);
                low += k * band;
                let high = s - low - damp * band;
                band += k * high;
                s = low;
            }
            if t.highpass > 0.0 {
                let fc = (t.highpass + t.highpass_sweep * secs).clamp(1.0, sr * 0.45);
                let rc = 1.0 / (tau * fc);
                let a = rc / (rc + 1.0 / sr);
                hp_out = a * (hp_out + s - hp_in);
                hp_in = s;
                s = hp_out;
            }
            if !delay.is_empty() {
                let n = delay.len();
                let d = ((t.phaser_ms + t.phaser_sweep * secs).max(0.0) / 1000.0 * sr) as usize;
                let back = delay[(at + n - d.min(n - 1)) % n];
                delay[at] = s;
                at = (at + 1) % n;
                s -= back;
            }
            // Per-note envelope: attack ramp, release ramp at the end.
            let env_a = if attack > 0 && i < attack {
                i as f32 / attack as f32
            } else {
                1.0
            };
            let left = note_len - i;
            let env_r = if release > 0 && left < release {
                left as f32 / release as f32
            } else {
                1.0
            };
            // Punch: the body starts louder than it ends.
            let punch = if t.punch > 0.0 {
                let u = (i.saturating_sub(attack) as f32 / body as f32).min(1.0);
                (1.0 + t.punch * (1.0 - u)) / (1.0 + t.punch)
            } else {
                1.0
            };
            out.push((s * t.gain * env_a.min(env_r) * punch).clamp(-1.0, 1.0));
        }
    }
    out.into()
}
