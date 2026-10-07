//! Section 15 (phase 5 item 1): "audio in the platform's preferred codec".
//! A release pack holds a WAV as Ogg Vorbis under its authored name, a
//! fraction of the size and close to what was authored, the same bytes on
//! every build; and the mixer loads it from the mounted packs, which are in
//! memory. Before this item a sound file was read from the disk, so a
//! release build had none.

use argus_player::pack::{self, BuildOptions};
use std::path::{Path, PathBuf};

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Temp(dir)
}

/// Half a second of a decaying chord in stereo, as 16-bit samples.
fn chime(rate: u32) -> Vec<i16> {
    let n = rate as usize / 2;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (-6.0 * t).exp();
        let tone = |f: f32| (std::f32::consts::TAU * f * t).sin();
        let l = 0.4 * env * (tone(440.0) + 0.5 * tone(660.0));
        let r = 0.4 * env * (tone(440.0) + 0.5 * tone(880.0));
        out.push((l * 32767.0) as i16);
        out.push((r * 32767.0) as i16);
    }
    out
}

/// A 16-bit stereo WAV file of `samples`.
fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes()); // PCM
    b.extend(2u16.to_le_bytes()); // channels
    b.extend(rate.to_le_bytes());
    b.extend((rate * 4).to_le_bytes());
    b.extend(4u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for s in samples {
        b.extend(s.to_le_bytes());
    }
    b
}

#[test]
fn a_wav_ships_as_vorbis_and_loads_from_the_packs() {
    let t = temp("release-audio");
    let root = t.join("chimes");
    std::fs::create_dir_all(root.join("assets/sfx")).unwrap();
    std::fs::write(
        root.join("argus.toml"),
        "name = \"chimes\"\nclock = \"continuous@60\"\n",
    )
    .unwrap();
    let authored = chime(44_100);
    let source = wav(44_100, &authored);
    std::fs::write(root.join("assets/sfx/chime.wav"), &source).unwrap();

    let built = pack::build(&root, &BuildOptions::default()).unwrap();
    let sounds = &built.report["sounds"];
    assert_eq!(sounds["encoded"], 1, "{}", built.report);
    let (wav_bytes, vorbis_bytes) = (
        sounds["wav_bytes"].as_u64().unwrap(),
        sounds["vorbis_bytes"].as_u64().unwrap(),
    );
    assert!(
        vorbis_bytes * 5 < wav_bytes,
        "{vorbis_bytes} bytes of Vorbis for {wav_bytes} of WAV"
    );

    // Under its own name, as Vorbis, and close to what was authored.
    let game = &built.packs[0];
    let (_, h, _) = game
        .files
        .iter()
        .find(|f| f.0 == "assets/sfx/chime.wav")
        .expect("the sound keeps its path");
    let ogg = &game.blobs[h];
    assert!(ogg.starts_with(b"OggS"));
    let d = argus_assets::decode_sound(ogg).unwrap();
    assert_eq!((d.rate, d.channels), (44_100, 2));
    let n = d.samples.len().min(authored.len());
    assert!(
        n + 2 * 4096 >= authored.len(),
        "{n} of {} samples",
        authored.len()
    );
    let (mut signal, mut noise) = (0f64, 0f64);
    for (a, b) in authored.iter().zip(&d.samples).take(n) {
        let a = *a as f64 / 32768.0;
        signal += a * a;
        noise += (a - *b as f64).powi(2);
    }
    let snr = 10.0 * (signal / noise.max(1e-12)).log10();
    assert!(snr > 20.0, "signal to noise {snr:.1} dB");

    // The same sound packs to the same blob, so a patch leaves it out.
    let again = pack::build(&root, &BuildOptions::default()).unwrap();
    assert!(again.packs[0].files.iter().any(|f| f.1 == *h));

    // The packs mounted as a release build mounts them: in memory.
    let data = t.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in &built.packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let game_root = t.join("game");
    pack::mount_dir(&data, &game_root, None).unwrap();
    assert!(
        !game_root.join("assets/sfx/chime.wav").exists(),
        "not on the disk"
    );
    let audio = argus_audio::AudioEngine::load(&game_root, 1).unwrap();
    assert!(
        audio.sound_names().contains(&"sfx/chime".to_string()),
        "{:?}",
        audio.sound_names()
    );
    assert!(audio.warnings.is_empty(), "{:?}", audio.warnings);

    // A sound authored as Ogg Vorbis plays in the project and ships as it is.
    std::fs::write(root.join("assets/sfx/bell.ogg"), ogg).unwrap();
    let audio = argus_audio::AudioEngine::load(&root, 1).unwrap();
    assert_eq!(audio.sound_names(), ["sfx/bell", "sfx/chime"]);
    assert!(audio.warnings.is_empty(), "{:?}", audio.warnings);
    let with_ogg = pack::build(&root, &BuildOptions::default()).unwrap();
    assert_eq!(with_ogg.report["sounds"]["encoded"], 1, "only the WAV");
    let bell = with_ogg.packs[0]
        .files
        .iter()
        .find(|f| f.0 == "assets/sfx/bell.ogg")
        .unwrap();
    assert_eq!(bell.1, *h, "the same bytes as the WAV's Vorbis");
}
