//! A sound described for someone who cannot hear it (an agent): how long
//! it is, how loud, where it is loudest and where it has faded, and how
//! its pitch and brightness move, as numbers and as a sentence. Measured
//! from the samples, so it says what plays, whatever made it.

use serde_json::{json, Value as Json};

/// What a sound is like, measured.
#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    /// Length in seconds.
    pub seconds: f32,
    /// Peak, dBFS.
    pub peak_db: f32,
    /// RMS over its audible part, dBFS.
    pub loudness_db: f32,
    /// When it is loudest (a 20 ms window's RMS), seconds.
    pub loudest_s: f32,
    /// When it has faded to a tenth of that (-20 dB) for good, seconds.
    pub faded_s: f32,
    /// The pitch every 20 ms where it has one: seconds, Hz.
    pub pitch: Vec<(f32, f32)>,
    /// Zero crossings a second, every 20 ms (high is bright or hissy).
    pub brightness: Vec<(f32, f32)>,
    /// The share of its audible windows that have a pitch.
    pub pitched: f32,
    /// All of it in a sentence.
    pub text: String,
}

const WIN_S: f32 = 0.02;

fn db(x: f32) -> f32 {
    if x <= 1e-9 {
        -120.0
    } else {
        20.0 * libm::log10f(x)
    }
}

fn rms(s: &[f32]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    libm::sqrtf(s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32)
}

/// The pitch of a window by normalised autocorrelation, 60 Hz to 4 kHz;
/// `None` when no lag repeats it well (noise, or silence).
fn pitch_of(w: &[f32], rate: u32) -> Option<f32> {
    let lo = (rate / 4000).max(2) as usize;
    let hi = ((rate / 60) as usize).min(w.len() / 2);
    if hi <= lo {
        return None;
    }
    let corr = |lag: usize| {
        let (mut num, mut a, mut b) = (0.0f32, 0.0f32, 0.0f32);
        for j in 0..w.len() - lag {
            num += w[j] * w[j + lag];
            a += w[j] * w[j];
            b += w[j + lag] * w[j + lag];
        }
        if a <= 1e-9 || b <= 1e-9 {
            0.0
        } else {
            num / libm::sqrtf(a * b)
        }
    };
    let r: Vec<f32> = (lo..=hi).map(corr).collect();
    // A pitch repeats: the correlation falls away and comes back. Noise
    // that holds each value a while (pitched noise) only falls.
    let dip = r.iter().position(|&x| x < 0.3)?;
    let best = r[dip..].iter().cloned().fold(0.0f32, f32::max);
    if best < 0.6 {
        return None;
    }
    // The first lag near the best, so an octave below never wins; then up
    // to that peak's top.
    let mut i = dip + r[dip..].iter().position(|&x| x >= best * 0.95)?;
    while i + 1 < r.len() && r[i + 1] > r[i] {
        i += 1;
    }
    // Between samples: the parabola through the peak and its neighbours.
    let lag = (lo + i) as f32
        + if i > 0 && i + 1 < r.len() {
            let (a, b, c) = (r[i - 1], r[i], r[i + 1]);
            let d = a - 2.0 * b + c;
            if d < -1e-9 {
                (0.5 * (a - c) / d).clamp(-0.5, 0.5)
            } else {
                0.0
            }
        } else {
            0.0
        };
    Some(rate as f32 / lag)
}

fn hz(f: f32) -> String {
    if f >= 1000.0 {
        format!("{:.1} kHz", f / 1000.0)
    } else {
        format!("{:.0} Hz", f)
    }
}

/// Describe mono `samples` at `rate`.
pub fn hear(samples: &[f32], rate: u32) -> Heard {
    let sr = rate as f32;
    let seconds = samples.len() as f32 / sr;
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let n = ((WIN_S * sr) as usize).max(1);
    let wins: Vec<(f32, &[f32])> = samples
        .chunks(n)
        .enumerate()
        .map(|(i, c)| (i as f32 * WIN_S, c))
        .collect();
    let levels: Vec<f32> = wins.iter().map(|(_, c)| rms(c)).collect();
    let (li, loud) =
        levels.iter().enumerate().fold(
            (0, 0.0f32),
            |(bi, b), (i, &x)| if x > b { (i, x) } else { (bi, b) },
        );
    let faded = levels
        .iter()
        .rposition(|&x| x >= loud * 0.1)
        .map(|i| (i as f32 + 1.0) * WIN_S)
        .unwrap_or(0.0)
        .min(seconds);
    let audible: Vec<usize> = (0..wins.len())
        .filter(|&i| levels[i] >= loud * 0.1)
        .collect();
    let body: Vec<f32> = audible
        .iter()
        .flat_map(|&i| wins[i].1.iter().copied())
        .collect();
    let mut pitch = Vec::new();
    let mut brightness = Vec::new();
    for &i in &audible {
        let (t, w) = wins[i];
        if let Some(p) = pitch_of(w, rate) {
            pitch.push((t, p));
        }
        let ups = w
            .windows(2)
            .filter(|x| (x[0] <= 0.0) != (x[1] <= 0.0))
            .count();
        brightness.push((t, ups as f32 / (w.len() as f32 / sr)));
    }
    let pitched = if audible.is_empty() {
        0.0
    } else {
        pitch.len() as f32 / audible.len() as f32
    };

    // The sentence.
    let mut text = format!("{seconds:.2} s long, peak {:.1} dB", db(peak));
    if loud <= 1e-6 {
        text.push_str("; silent");
    } else {
        text.push_str(&format!(
            "; loudest at {:.2} s, faded to a tenth by {faded:.2} s",
            li as f32 * WIN_S
        ));
        if pitched >= 0.5 && !pitch.is_empty() {
            let (t0, f0) = pitch[0];
            let (t1, f1) = *pitch.last().expect("a pitch");
            let (tmax, fmax) = pitch
                .iter()
                .cloned()
                .fold((t0, f0), |m, x| if x.1 > m.1 { x } else { m });
            let (tmin, fmin) = pitch
                .iter()
                .cloned()
                .fold((t0, f0), |m, x| if x.1 < m.1 { x } else { m });
            // A step: one window to the next by more than two semitones,
            // the pitch steady on either side (a slide moves every window).
            let steady = |a: f32, b: f32| (1.0 / 1.04..=1.04).contains(&(b / a));
            let step = (1..pitch.len())
                .find(|&k| {
                    let r = pitch[k].1 / pitch[k - 1].1;
                    !(1.0 / 1.15..=1.15).contains(&r)
                        && (k < 2 || steady(pitch[k - 2].1, pitch[k - 1].1))
                        && (k + 1 >= pitch.len() || steady(pitch[k].1, pitch[k + 1].1))
                })
                .map(|k| [pitch[k - 1], pitch[k]]);
            text.push_str("; a note ");
            if fmax / fmin < 1.03 {
                text.push_str(&format!("steady near {}", hz(f0)));
            } else {
                text.push_str(&format!("starting near {} at {t0:.2} s", hz(f0)));
                if let Some(w) = step {
                    let verb = if w[1].1 > w[0].1 {
                        "jumping up"
                    } else {
                        "dropping"
                    };
                    text.push_str(&format!(", {verb} to {} at {:.2} s", hz(w[1].1), w[1].0));
                }
                if tmax != t0 && tmax != t1 && fmax > f0.max(f1) * 1.03 {
                    text.push_str(&format!(", highest {} at {tmax:.2} s", hz(fmax)));
                }
                if tmin != t0 && tmin != t1 && fmin < f0.min(f1) / 1.03 {
                    text.push_str(&format!(", lowest {} at {tmin:.2} s", hz(fmin)));
                }
                let way = if f1 > f0 * 1.03 {
                    "higher"
                } else if f1 < f0 / 1.03 {
                    "lower"
                } else {
                    "back where it began"
                };
                text.push_str(&format!(", ending near {} at {t1:.2} s ({way})", hz(f1)));
            }
        } else {
            let mean = brightness.iter().map(|b| b.1).sum::<f32>() / brightness.len().max(1) as f32;
            let tone = if mean > 8000.0 {
                "hissy, bright"
            } else if mean > 2500.0 {
                "bright"
            } else if mean > 800.0 {
                "dull"
            } else {
                "deep, rumbling"
            };
            text.push_str(&format!(
                "; noise, {tone} (about {} crossings a second)",
                mean.round()
            ));
            if let (Some(a), Some(b)) = (brightness.first(), brightness.last()) {
                if b.1 < a.1 * 0.6 {
                    text.push_str(", darkening as it goes");
                } else if b.1 > a.1 * 1.6 {
                    text.push_str(", brightening as it goes");
                }
            }
        }
    }
    Heard {
        seconds,
        peak_db: db(peak),
        loudness_db: db(rms(&body)),
        loudest_s: li as f32 * WIN_S,
        faded_s: faded,
        pitch,
        brightness,
        pitched,
        text,
    }
}

impl Heard {
    /// As JSON, rounded for reading: the pitch and brightness every 40 ms.
    pub fn to_json(&self) -> Json {
        let r2 = |x: f32| (x as f64 * 100.0).round() / 100.0;
        let r0 = |x: f32| x.round() as f64;
        json!({
            "text": self.text,
            "seconds": r2(self.seconds),
            "peak_db": (self.peak_db as f64 * 10.0).round() / 10.0,
            "loudness_db": (self.loudness_db as f64 * 10.0).round() / 10.0,
            "loudest_s": r2(self.loudest_s),
            "faded_s": r2(self.faded_s),
            "pitched": r2(self.pitched),
            "pitch_hz": self.pitch.iter().step_by(2).map(|(t, f)| json!([r2(*t), r0(*f)])).collect::<Vec<_>>(),
            "crossings_per_s": self.brightness.iter().step_by(2).map(|(t, f)| json!([r2(*t), r0(*f)])).collect::<Vec<_>>(),
        })
    }
}
