//! The system's audio output (cpal). The callback thread pulls blocks
//! straight from the shared mixer, so audio runs in real time even on a
//! discrete scene where nothing ticks between turns. While `hold` is set
//! (the game is not playing) it sends silence and leaves the mixer where
//! it is, so the music goes on from the same place when play does.

use crate::mixer::Mixer;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// An open output stream. Dropping it closes the device.
pub struct Output {
    _stream: cpal::Stream,
    /// Device name.
    pub name: String,
    /// Sample rate the device runs at.
    pub rate: u32,
    /// Channels the device has (the mixer's stereo is spread over them).
    pub channels: u16,
}

/// The rate the default device will run at, preferring `want`.
pub fn probe(want: u32) -> Result<u32, String> {
    let (_, cfg) = pick(want)?;
    Ok(cfg.sample_rate().0)
}

fn pick(want: u32) -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no audio output device".to_string())?;
    // A Mac's device runs at the rate the system set it to, and a stream
    // at another rate makes CoreAudio change it for every program on the
    // Mac (and some devices refuse): the mixer takes the device's rate.
    if cfg!(target_os = "macos") {
        if let Ok(c) = device.default_output_config() {
            if c.channels() >= 2 {
                return Ok((device, c));
            }
        }
    }
    let mut chosen = None;
    if let Ok(ranges) = device.supported_output_configs() {
        for r in ranges {
            if r.channels() >= 2
                && r.sample_format() == cpal::SampleFormat::F32
                && r.min_sample_rate().0 <= want
                && r.max_sample_rate().0 >= want
            {
                chosen = Some(r.with_sample_rate(cpal::SampleRate(want)));
                break;
            }
        }
    }
    let cfg = match chosen {
        Some(c) => c,
        None => device.default_output_config().map_err(|e| e.to_string())?,
    };
    Ok((device, cfg))
}

/// Open the default device and start pulling from `mixer`, unless `hold`.
pub fn open(mixer: Arc<Mutex<Mixer>>, want: u32, hold: Arc<AtomicBool>) -> Result<Output, String> {
    let (device, mut supported) = pick(want)?;
    let name = device.name().unwrap_or_else(|_| "audio device".to_string());
    let start = |s: &cpal::SupportedStreamConfig| {
        let config: cpal::StreamConfig = s.config();
        match s.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, mixer.clone(), hold.clone()),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, mixer.clone(), hold.clone()),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, mixer.clone(), hold.clone()),
            other => Err(format!("unsupported sample format {other:?}")),
        }
    };
    let stream = match start(&supported) {
        Ok(s) => s,
        // The rate asked for, refused: the device's own, once.
        Err(e) => match device.default_output_config() {
            Ok(d) if d != supported => {
                let s = start(&d).map_err(|e2| format!("{e}; at the device's own rate, {e2}"))?;
                supported = d;
                s
            }
            _ => return Err(e),
        },
    };
    let rate = supported.sample_rate().0;
    let channels = supported.channels();
    stream.play().map_err(|e| e.to_string())?;
    Ok(Output {
        _stream: stream,
        name,
        rate,
        channels,
    })
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mixer: Arc<Mutex<Mixer>>,
    hold: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let ch = config.channels as usize;
    let mut stereo: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                if hold.load(Ordering::Relaxed) {
                    data.fill(T::from_sample(0.0f32));
                    return;
                }
                let frames = data.len() / ch;
                stereo.resize(frames * 2, 0.0);
                match mixer.lock() {
                    Ok(mut m) => m.render(&mut stereo),
                    Err(p) => p.into_inner().render(&mut stereo),
                }
                for (f, out) in data.chunks_mut(ch).enumerate() {
                    for (c, s) in out.iter_mut().enumerate() {
                        let v = if c < 2 { stereo[f * 2 + c] } else { 0.0 };
                        *s = T::from_sample(v);
                    }
                }
            },
            |e| eprintln!("argus audio: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}
