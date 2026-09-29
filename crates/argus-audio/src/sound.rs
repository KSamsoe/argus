//! Sample data: WAV files decoded to mono at the mixer rate, and described
//! tones synthesised the same way, so every variation is one `Clip`.

use crate::config::{Tone, Wave};
use argus_ecs::{ArgusError, Result};
use std::path::Path;
use std::sync::Arc;

/// Mono samples at the mixer rate.
pub type Clip = Arc<[f32]>;

/// Decode a WAV (8/16/24/32-bit integer or 32-bit float, any channel
/// count, any rate) to mono at `rate`.
pub fn load_wav(path: &Path, rate: u32) -> Result<Clip> {
    let e = |m: String| {
        ArgusError::new(crate::codes::BAD_SOUND, format!("{}: {m}", path.display()))
            .hint("Sounds are WAV files under assets/; a described tone in audio.toml works as a placeholder.")
    };
    let mut r = hound::WavReader::open(path).map_err(|x| e(x.to_string()))?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()
            .map_err(|x| e(x.to_string()))?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<std::result::Result<_, _>>()
                .map_err(|x| e(x.to_string()))?
        }
    };
    let mono: Vec<f32> = interleaved
        .chunks(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok(resample(&mono, spec.sample_rate, rate).into())
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
/// fixed seed, so the same description is the same samples everywhere.
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
    let mut phase = 0.0f32;
    for &f0 in &notes {
        for i in 0..note_len {
            let secs = i as f32 / sr;
            let f = (f0 + t.slide * secs).max(0.0);
            phase = (phase + f / sr).fract();
            let s = match t.wave {
                Wave::Sine => (phase * std::f32::consts::TAU).sin(),
                Wave::Square => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Saw => 2.0 * phase - 1.0,
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Noise => {
                    noise ^= noise << 13;
                    noise ^= noise >> 17;
                    noise ^= noise << 5;
                    (noise as f32 / u32::MAX as f32) * 2.0 - 1.0
                }
            };
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
            out.push(s * t.gain * env_a.min(env_r));
        }
    }
    out.into()
}
