//! The sound-effect synthesiser (asset generation, item 0) and its
//! generators (item 1): every new control is neutral by default, so a tone
//! that names none sounds as tones always have; each control does what it
//! says; a kind and a seed make one tone, its TOML reads back as the same
//! tone, and the same tone is the same samples.

use argus_audio::config::{parse_sound_sidecar, Source, Tone, Wave};
use argus_audio::gen;
use argus_audio::sound::synth;

const RATE: u32 = 48_000;

/// The synthesiser as it was before the effect controls (0.0.4), to hold
/// the new one to it.
fn old_synth(t: &Tone, rate: u32) -> Vec<f32> {
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
    let mut out = Vec::new();
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
    out
}

fn tone(wave: Wave) -> Tone {
    Tone {
        wave,
        freq: 330.0,
        ms: 200.0,
        slide: 400.0,
        ..Tone::default()
    }
}

#[test]
fn a_tone_without_the_new_controls_sounds_as_before() {
    let mut cases = Vec::new();
    for w in [
        Wave::Sine,
        Wave::Square,
        Wave::Saw,
        Wave::Triangle,
        Wave::Noise,
    ] {
        cases.push(tone(w));
        cases.push(Tone {
            notes: vec![110.0, 131.0, 98.0],
            note_ms: 90.0,
            attack_ms: 10.0,
            release_ms: 30.0,
            ..tone(w)
        });
    }
    for t in &cases {
        let (new, old) = (synth(t, RATE), old_synth(t, RATE));
        assert_eq!(new.len(), old.len());
        if t.wave == Wave::Sine {
            // Sines are libm's now, the same on every platform: within a
            // few ulps of the platform's.
            let worst = new
                .iter()
                .zip(&old)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(worst < 1e-6, "{worst}");
        } else {
            assert!(
                new.iter()
                    .zip(&old)
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
                "{t:?}"
            );
        }
    }
}

/// Upward zero crossings a second in `s[from..to]` seconds: the pitch.
fn pitch(s: &[f32], from: f32, to: f32) -> f32 {
    let (a, b) = ((from * RATE as f32) as usize, (to * RATE as f32) as usize);
    let ups = s[a..b]
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    ups as f32 / (to - from)
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

/// How rough: the size of a step from one sample to the next against the
/// level, both as RMS (high frequencies step far).
fn rough(s: &[f32]) -> f32 {
    let d: Vec<f32> = s.windows(2).map(|w| w[1] - w[0]).collect();
    rms(&d) / rms(s).max(1e-9)
}

#[test]
fn each_control_does_what_it_says() {
    let base = Tone {
        wave: Wave::Square,
        freq: 400.0,
        ms: 400.0,
        attack_ms: 0.0,
        release_ms: 0.0,
        ..Tone::default()
    };
    // A jump: twice the pitch after 200 ms.
    let s = synth(
        &Tone {
            jump: 2.0,
            jump_ms: 200.0,
            ..base.clone()
        },
        RATE,
    );
    let (p0, p1) = (pitch(&s, 0.05, 0.19), pitch(&s, 0.21, 0.39));
    assert!(
        (p0 - 400.0).abs() < 15.0 && (p1 - 800.0).abs() < 15.0,
        "{p0} {p1}"
    );

    // Repeat: the slide starts over every 100 ms.
    let s = synth(
        &Tone {
            slide: 2000.0,
            repeat_ms: 100.0,
            ..base.clone()
        },
        RATE,
    );
    let (a, b) = (pitch(&s, 0.01, 0.05), pitch(&s, 0.21, 0.25));
    assert!((a - b).abs() < 30.0, "{a} {b}");

    // Duty: a quarter of each period high.
    let s = synth(
        &Tone {
            duty: 0.25,
            ..base.clone()
        },
        RATE,
    );
    let high = s.iter().filter(|x| **x > 0.0).count() as f32 / s.len() as f32;
    assert!((high - 0.25).abs() < 0.01, "{high}");

    // Vibrato: the pitch swings.
    let s = synth(
        &Tone {
            vibrato: 0.3,
            vibrato_hz: 5.0,
            ..base.clone()
        },
        RATE,
    );
    let (lo, hi) = (pitch(&s, 0.13, 0.17), pitch(&s, 0.03, 0.07));
    assert!(hi > lo * 1.3, "{hi} {lo}");

    // A low-pass smooths; a high-pass takes a bass note's body away.
    let plain = synth(&base, RATE);
    let low = synth(
        &Tone {
            lowpass: 600.0,
            ..base.clone()
        },
        RATE,
    );
    assert!(
        rough(&low) < rough(&plain) * 0.5,
        "{} {}",
        rough(&low),
        rough(&plain)
    );
    let bass = Tone {
        wave: Wave::Sine,
        freq: 60.0,
        ..base.clone()
    };
    let high = synth(
        &Tone {
            highpass: 2000.0,
            ..bass.clone()
        },
        RATE,
    );
    assert!(rms(&high) < rms(&synth(&bass, RATE)) * 0.2);

    // Punch: the body's start is louder than its end.
    let s = synth(
        &Tone {
            punch: 0.8,
            ..base.clone()
        },
        RATE,
    );
    let n = s.len();
    assert!(rms(&s[..n / 4]) > rms(&s[3 * n / 4..]) * 1.4);

    // Pitched noise changes value at the pitch, white noise every sample.
    let white = synth(
        &Tone {
            wave: Wave::Noise,
            ..base.clone()
        },
        RATE,
    );
    let rumble = synth(
        &Tone {
            wave: Wave::Noise,
            noise_pitch: true,
            freq: 200.0,
            ..base.clone()
        },
        RATE,
    );
    let changes = |s: &[f32]| s.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(
        changes(&rumble) * 20 < changes(&white),
        "{} {}",
        changes(&rumble),
        changes(&white)
    );

    // A phaser changes the sound; the level stays within the mix's range.
    let s = synth(
        &Tone {
            phaser_ms: 1.0,
            phaser_sweep: 4.0,
            gain: 1.0,
            ..base.clone()
        },
        RATE,
    );
    assert!(
        *s != *synth(
            &Tone {
                gain: 1.0,
                ..base.clone()
            },
            RATE
        )
    );
    assert!(s.iter().all(|x| x.abs() <= 1.0));
}

fn read_back(text: &str) -> Tone {
    let def = parse_sound_sidecar("sfx/x", "assets/sfx/x.toml", text).unwrap();
    match &def.variations[..] {
        [Source::Tone(t)] => t.clone(),
        v => panic!("{v:?}"),
    }
}

#[test]
fn a_kind_and_a_seed_make_one_tone_and_its_toml_reads_back_as_it() {
    for (kind, _) in gen::KINDS {
        let a = gen::generate(kind, 1).unwrap();
        assert_eq!(a, gen::generate(kind, 1).unwrap(), "{kind}: the same seed");
        assert_ne!(a, gen::generate(kind, 2).unwrap(), "{kind}: another seed");
        let text = gen::sidecar("sfx/x", kind, 1, None, &a);
        assert_eq!(read_back(&text), a, "{kind}:\n{text}");
        let s = synth(&a, RATE);
        assert!(!s.is_empty() && rms(&s) > 0.01, "{kind} is silent");
        assert!(s.iter().all(|x| x.abs() <= 1.0), "{kind}");
    }
    // A variation moves what the tone uses, by up to the share.
    let a = gen::generate("laser", 4).unwrap();
    let b = gen::vary(&a, 0.1, 9);
    assert_ne!(a, b);
    assert!(
        (b.freq / a.freq - 1.0).abs() <= 0.11,
        "{} {}",
        a.freq,
        b.freq
    );
    assert_eq!(b.wave, a.wave);
    assert_eq!(
        read_back(&gen::sidecar(
            "sfx/y",
            "laser",
            9,
            Some(("sfx/laser", 0.1)),
            &b
        )),
        b
    );
    let e = gen::generate("boing", 1).unwrap_err();
    assert!(e.contains("pickup, laser"), "{e}");
}

/// The same tone is the same samples: these numbers are pinned, so a
/// change to the synthesiser or the generators shows here, and the test
/// says so on any machine that runs it (the gate runs it on Linux too).
#[test]
fn generated_tones_are_pinned_samples() {
    let fnv = |s: &[f32]| {
        s.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, x| {
            (h ^ x.to_bits() as u64).wrapping_mul(0x0100_0000_01b3)
        })
    };
    let got: Vec<(String, u64)> = ["pickup", "laser", "explosion", "jump"]
        .iter()
        .map(|k| {
            (
                k.to_string(),
                fnv(&synth(&gen::generate(k, 7).unwrap(), RATE)),
            )
        })
        .collect();
    let want: Vec<(String, u64)> = PINNED.iter().map(|(k, h)| (k.to_string(), *h)).collect();
    assert_eq!(got, want);
}

const PINNED: [(&str, u64); 4] = [
    ("pickup", 18112830117366128900),
    ("laser", 16709331222200757669),
    ("explosion", 8950501901046794229),
    ("jump", 9880927612308710788),
];

/// A sound described for someone who cannot hear it: a jump rises, a
/// laser falls, a coin with a second note jumps up, an explosion is
/// noise that darkens, silence is silent.
#[test]
fn a_sound_is_described_in_numbers_and_words() {
    use argus_audio::hear::hear;
    let up = synth(
        &Tone {
            wave: Wave::Square,
            freq: 300.0,
            slide: 1500.0,
            ms: 300.0,
            ..Tone::default()
        },
        RATE,
    );
    let h = hear(&up, RATE);
    assert!(
        h.text.contains("starting near 3") && h.text.contains("(higher)"),
        "{}",
        h.text
    );
    assert!((h.seconds - 0.3).abs() < 0.01);

    let down = synth(&gen::generate("laser", 3).unwrap(), RATE);
    let h = hear(&down, RATE);
    assert!(h.text.contains("(lower)"), "{}", h.text);

    let coin = synth(
        &Tone {
            wave: Wave::Square,
            freq: 800.0,
            ms: 250.0,
            jump: 1.5,
            jump_ms: 80.0,
            release_ms: 100.0,
            ..Tone::default()
        },
        RATE,
    );
    let h = hear(&coin, RATE);
    assert!(
        h.text.contains("jumping up to 1.2 kHz at 0.08"),
        "{}",
        h.text
    );

    let boom = synth(&gen::generate("explosion", 3).unwrap(), RATE);
    let h = hear(&boom, RATE);
    assert!(h.text.contains("noise"), "{}", h.text);

    let quiet = hear(&vec![0.0; 4800], RATE);
    assert!(quiet.text.contains("silent"), "{}", quiet.text);
    for (k, _) in gen::KINDS {
        let h = hear(&synth(&gen::generate(k, 1).unwrap(), RATE), RATE);
        println!("{k}: {}", h.text);
    }
}

/// For a person to listen to the kinds: every kind at seeds 1 to 3 as
/// WAVs in $ARGUS_WAVS (`ARGUS_WAVS=dir cargo test -p argus-audio --test
/// synth -- --ignored`).
#[test]
#[ignore]
fn write_the_kinds_as_wavs() {
    let Ok(dir) = std::env::var("ARGUS_WAVS") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    for (kind, _) in gen::KINDS {
        for seed in 1..=3 {
            let s = synth(&gen::generate(kind, seed).unwrap(), RATE);
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let path = std::path::Path::new(&dir).join(format!("{kind}-{seed}.wav"));
            let mut w = hound::WavWriter::create(path, spec).unwrap();
            for x in s.iter() {
                w.write_sample((x * 32767.0) as i16).unwrap();
            }
            w.finalize().unwrap();
        }
    }
}

/// A sound that clips says so, and a note's brightness is in the sentence.
#[test]
fn a_description_says_when_a_sound_clips_and_how_its_brightness_goes() {
    use argus_audio::hear::hear;
    let loud = synth(
        &Tone {
            wave: Wave::Square,
            freq: 220.0,
            ms: 300.0,
            gain: 1.0,
            punch: 0.0,
            ..Tone::default()
        },
        RATE,
    );
    let h = hear(&loud, RATE);
    assert!(h.text.contains("it clips"), "{}", h.text);
    let dull = synth(
        &Tone {
            wave: Wave::Saw,
            freq: 300.0,
            ms: 400.0,
            lowpass: 6000.0,
            lowpass_sweep: -14000.0,
            ..Tone::default()
        },
        RATE,
    );
    let h = hear(&dull, RATE);
    assert!(h.text.contains("grows duller"), "{}", h.text);
    assert!(!h.text.contains("clips"), "{}", h.text);
}
