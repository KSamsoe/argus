//! Sound effects made from a kind and a seed (the asset generators, after
//! sfxr's): each kind is ranges for a tone's controls, and the seed picks
//! within them, so `jump` with seed 2 is another jump and the same seed is
//! the same tone. What comes out is a tone, written as TOML a person or an
//! agent goes on to change; nothing here is random at play time.

use crate::config::{Tone, Wave};
use argus_sim::rng::{stream, Pcg32};

/// The kinds, each with what it is for.
pub const KINDS: [(&str, &str); 9] = [
    (
        "pickup",
        "a coin or a gem: a bright note, often a second one higher",
    ),
    ("laser", "a shot: a high note falling fast"),
    ("explosion", "a blast: low pitched noise with a long tail"),
    ("powerup", "something gained: a note rising, often in steps"),
    ("hit", "a blow landing or a hurt: a short note falling"),
    ("jump", "a jump: a short note rising"),
    ("blip", "a menu move or a select: a short plain note"),
    ("step", "a footstep: a soft burst of dull noise"),
    ("click", "a button: a very short high tick"),
];

struct R(Pcg32);

impl R {
    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.0.next_f32()
    }
    fn chance(&mut self, p: f32) -> bool {
        self.0.chance(p)
    }
    fn wave(&mut self, of: &[Wave]) -> Wave {
        *self.0.pick(of).expect("a wave")
    }
}

/// A tone of `kind`, picked by `seed`. `Err` names the kinds.
pub fn generate(kind: &str, seed: u64) -> Result<Tone, String> {
    let mut r = R(stream(seed, &format!("sound.{kind}")));
    let mut t = Tone {
        attack_ms: 0.0,
        ..Tone::default()
    };
    match kind {
        "pickup" => {
            t.wave = r.wave(&[Wave::Square, Wave::Square, Wave::Sine, Wave::Triangle]);
            t.freq = r.f(600.0, 1600.0);
            t.ms = r.f(110.0, 300.0);
            t.release_ms = t.ms * r.f(0.5, 0.8);
            t.punch = r.f(0.3, 0.6);
            if r.chance(0.6) {
                t.jump = r.f(1.25, 2.0);
                t.jump_ms = r.f(40.0, 90.0);
            }
        }
        "laser" => {
            t.wave = r.wave(&[Wave::Square, Wave::Saw, Wave::Sine]);
            t.freq = r.f(900.0, 2200.0);
            t.ms = r.f(100.0, 280.0);
            // Down to a share of where it began, the fall easing off.
            let secs = t.ms / 1000.0;
            t.slide = (t.freq * r.f(0.15, 0.45) - t.freq) / secs;
            t.slide_accel = -t.slide / secs * r.f(0.0, 0.8);
            t.release_ms = t.ms * r.f(0.4, 0.8);
            t.punch = r.f(0.0, 0.4);
            if t.wave == Wave::Square {
                t.duty = r.f(0.2, 0.5);
                t.duty_sweep = r.f(-0.8, 0.8);
            }
            if r.chance(0.35) {
                t.highpass = r.f(100.0, 600.0);
            }
            if r.chance(0.4) {
                t.phaser_ms = r.f(0.5, 4.0);
                t.phaser_sweep = r.f(-5.0, 5.0);
            }
        }
        "explosion" => {
            t.wave = Wave::Noise;
            t.noise_pitch = true;
            t.freq = r.f(300.0, 2000.0);
            t.ms = r.f(350.0, 900.0);
            t.slide = (t.freq * r.f(0.1, 0.35) - t.freq) / (t.ms / 1000.0);
            t.attack_ms = r.f(0.0, 8.0);
            t.release_ms = t.ms * r.f(0.7, 0.95);
            t.punch = r.f(0.3, 0.8);
            t.gain = 0.6;
            t.lowpass = r.f(1200.0, 4000.0);
            t.lowpass_sweep = -t.lowpass * r.f(0.5, 1.2);
            if r.chance(0.3) {
                t.vibrato = r.f(0.1, 0.4);
                t.vibrato_hz = r.f(8.0, 20.0);
            }
        }
        "powerup" => {
            t.wave = r.wave(&[Wave::Square, Wave::Saw, Wave::Triangle]);
            t.freq = r.f(300.0, 800.0);
            t.ms = r.f(250.0, 600.0);
            t.slide = r.f(1.0, 4.0) * t.freq;
            t.release_ms = t.ms * r.f(0.3, 0.6);
            if r.chance(0.5) {
                t.repeat_ms = r.f(60.0, 150.0);
            }
            if r.chance(0.5) {
                t.vibrato = r.f(0.03, 0.15);
                t.vibrato_hz = r.f(10.0, 25.0);
            }
        }
        "hit" => {
            t.wave = r.wave(&[Wave::Square, Wave::Saw, Wave::Noise]);
            t.freq = r.f(150.0, 600.0);
            t.ms = r.f(70.0, 200.0);
            t.slide = (t.freq * r.f(0.3, 0.7) - t.freq) / (t.ms / 1000.0);
            t.release_ms = t.ms * r.f(0.5, 0.9);
            t.punch = r.f(0.2, 0.6);
            if t.wave == Wave::Noise {
                t.noise_pitch = true;
            }
            if r.chance(0.5) {
                t.highpass = r.f(80.0, 400.0);
            }
        }
        "jump" => {
            t.wave = Wave::Square;
            t.duty = r.f(0.25, 0.5);
            t.freq = r.f(250.0, 600.0);
            t.ms = r.f(140.0, 300.0);
            t.slide = r.f(2.0, 5.0) * t.freq;
            t.release_ms = t.ms * r.f(0.4, 0.8);
            if r.chance(0.35) {
                t.highpass = r.f(100.0, 400.0);
            } else if r.chance(0.5) {
                t.lowpass = r.f(2000.0, 6000.0);
            }
        }
        "blip" => {
            t.wave = r.wave(&[Wave::Square, Wave::Saw, Wave::Triangle]);
            if t.wave == Wave::Square {
                t.duty = r.f(0.3, 0.5);
            }
            t.freq = r.f(500.0, 1400.0);
            t.ms = r.f(40.0, 90.0);
            t.release_ms = t.ms * r.f(0.2, 0.5);
            if r.chance(0.3) {
                t.highpass = r.f(100.0, 500.0);
            }
        }
        "step" => {
            t.wave = Wave::Noise;
            t.ms = r.f(40.0, 90.0);
            t.attack_ms = r.f(0.5, 2.0);
            t.release_ms = t.ms * r.f(0.6, 0.9);
            t.gain = r.f(0.85, 1.0);
            t.lowpass = r.f(500.0, 1400.0);
            t.punch = r.f(0.3, 0.7);
        }
        "click" => {
            t.wave = r.wave(&[Wave::Square, Wave::Triangle]);
            t.freq = r.f(1500.0, 3200.0);
            t.ms = r.f(8.0, 22.0);
            t.release_ms = t.ms * 0.6;
            t.gain = 0.4;
        }
        _ => {
            let names: Vec<&str> = KINDS.iter().map(|(k, _)| *k).collect();
            return Err(format!(
                "there is no sound kind {kind:?}; the kinds are {}",
                names.join(", ")
            ));
        }
    }
    Ok(round(t))
}

/// Another take on `t`: every control it uses moved by up to `share` of
/// itself (0.1 is ten percent), picked by `seed`; its wave, its notes, and
/// what it leaves off stay as they are.
pub fn vary(t: &Tone, share: f32, seed: u64) -> Tone {
    let mut r = R(stream(seed, "sound.vary"));
    let k = share.clamp(0.0, 1.0);
    let mut m = |v: f32| v * (1.0 + r.f(-k, k));
    let mut o = t.clone();
    o.freq = m(t.freq);
    o.ms = m(t.ms).max(1.0);
    o.release_ms = m(t.release_ms).min(o.ms);
    o.slide = m(t.slide);
    o.slide_accel = m(t.slide_accel);
    o.punch = m(t.punch).clamp(0.0, 1.0);
    if t.vibrato != 0.0 {
        o.vibrato = m(t.vibrato).clamp(0.0, 1.0);
        o.vibrato_hz = m(t.vibrato_hz);
    }
    if t.jump != 1.0 {
        o.jump = m(t.jump);
        o.jump_ms = m(t.jump_ms);
    }
    if t.duty != 0.5 || t.duty_sweep != 0.0 {
        o.duty = m(t.duty).clamp(0.02, 0.98);
        o.duty_sweep = m(t.duty_sweep);
    }
    o.repeat_ms = m(t.repeat_ms);
    o.lowpass = m(t.lowpass);
    o.lowpass_sweep = m(t.lowpass_sweep);
    o.highpass = m(t.highpass);
    o.phaser_ms = m(t.phaser_ms);
    o.phaser_sweep = m(t.phaser_sweep);
    round(o)
}

/// Every number to a few figures, so the TOML reads as a person wrote it.
fn round(mut t: Tone) -> Tone {
    // In f64, then to the nearest f32, which prints as the short number
    // (0.35, not 0.35000002).
    let r = |v: f32| {
        if v == 0.0 {
            return 0.0;
        }
        let v = v as f64;
        let mag = libm::floor(libm::log10(v.abs()));
        let step = libm::pow(10.0, mag - 2.0);
        (libm::round(v / step) * step) as f32
    };
    for v in [
        &mut t.freq,
        &mut t.ms,
        &mut t.attack_ms,
        &mut t.release_ms,
        &mut t.gain,
        &mut t.slide,
        &mut t.punch,
        &mut t.slide_accel,
        &mut t.vibrato,
        &mut t.vibrato_hz,
        &mut t.jump,
        &mut t.jump_ms,
        &mut t.duty,
        &mut t.duty_sweep,
        &mut t.repeat_ms,
        &mut t.lowpass,
        &mut t.lowpass_sweep,
        &mut t.highpass,
        &mut t.highpass_sweep,
        &mut t.phaser_ms,
        &mut t.phaser_sweep,
    ] {
        *v = r(*v);
    }
    t
}

/// The tone as the keys of a `[tone]` table, one a line: the wave, its
/// length and envelope, and every other control it uses (one at its
/// default is left out).
pub fn tone_toml(t: &Tone) -> String {
    let d = Tone::default();
    let wave = match t.wave {
        Wave::Sine => "sine",
        Wave::Square => "square",
        Wave::Saw => "saw",
        Wave::Triangle => "triangle",
        Wave::Noise => "noise",
    };
    let mut out = format!("wave = \"{wave}\"\n");
    let num = |v: f32| {
        let s = format!("{v}");
        if s.contains('.') || s.contains('e') {
            s
        } else {
            format!("{s}.0")
        }
    };
    let mut put = |key: &str, v: f32, def: f32, always: bool| {
        if always || v != def {
            out.push_str(&format!("{key} = {}\n", num(v)));
        }
    };
    let pitched = t.wave != Wave::Noise || t.noise_pitch;
    put("freq", t.freq, d.freq, pitched);
    put("ms", t.ms, d.ms, true);
    put("attack_ms", t.attack_ms, d.attack_ms, true);
    put("release_ms", t.release_ms, d.release_ms, true);
    put("gain", t.gain, d.gain, true);
    put("punch", t.punch, d.punch, false);
    put("slide", t.slide, d.slide, false);
    put("slide_accel", t.slide_accel, d.slide_accel, false);
    put("jump", t.jump, d.jump, false);
    if t.jump != d.jump {
        put("jump_ms", t.jump_ms, d.jump_ms, true);
    }
    put("vibrato", t.vibrato, d.vibrato, false);
    if t.vibrato != d.vibrato {
        put("vibrato_hz", t.vibrato_hz, d.vibrato_hz, true);
    }
    if t.wave == Wave::Square {
        put("duty", t.duty, d.duty, false);
        put("duty_sweep", t.duty_sweep, d.duty_sweep, false);
    }
    put("repeat_ms", t.repeat_ms, d.repeat_ms, false);
    put("lowpass", t.lowpass, d.lowpass, false);
    if t.lowpass != d.lowpass {
        put("lowpass_sweep", t.lowpass_sweep, d.lowpass_sweep, false);
        put("resonance", t.resonance, d.resonance, false);
    }
    put("highpass", t.highpass, d.highpass, false);
    if t.highpass != d.highpass {
        put("highpass_sweep", t.highpass_sweep, d.highpass_sweep, false);
    }
    put("phaser_ms", t.phaser_ms, d.phaser_ms, false);
    if t.phaser_ms != d.phaser_ms {
        put("phaser_sweep", t.phaser_sweep, d.phaser_sweep, false);
    }
    if t.noise_pitch && t.wave == Wave::Noise {
        out.push_str("noise_pitch = true\n");
    }
    out
}

/// A whole sound sidecar for a generated tone (`assets/<name>.toml`, type
/// sound), headed by what made it, as the command that makes it again.
pub fn sidecar(name: &str, kind: &str, seed: u64, from: Option<(&str, f32)>, t: &Tone) -> String {
    let (what, made) = match from {
        Some((src, share)) => (
            format!("A variation of {src}"),
            format!("argus generate name={name} from={src} vary={share} seed={seed}"),
        ),
        None => {
            let what = KINDS
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, d)| {
                    let mut c = d.chars();
                    c.next()
                        .map(|f| f.to_uppercase().chain(c).collect::<String>())
                        .unwrap_or_default()
                })
                .unwrap_or_else(|| "A sound".to_string());
            (
                what,
                format!("argus generate name={name} sound={kind} seed={seed}"),
            )
        }
    };
    // Interface sounds play the same wherever the camera is.
    let spatial = if matches!(kind, "blip" | "click") {
        "spatial = false\n"
    } else {
        ""
    };
    format!(
        "# {what}.\n# Made by: {made}\n# Change any number below, and the game plays the change when the\n\
         # file is saved. What each key does: argus docs files section=audio.toml\n\
         type = \"sound\"\nbus = \"sfx\"\n{spatial}\n[tone]\n{}",
        tone_toml(t)
    )
}
