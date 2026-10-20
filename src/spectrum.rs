//! Frequency analysis of a song's preview clip for the player's visualizer. Full songs are
//! DRM-protected and can't be read, but the 30-second preview is the song's real audio.

use std::path::Path;

use anyhow::{Result, anyhow};
use rodio::{Decoder, Source};

/// Bars across the visualizer.
pub const BANDS: usize = 64;
/// Analysis frames per second.
pub const FPS: f32 = 30.;
const FFT: usize = 2048;

/// Per-frame band levels (0–1), `FPS` frames per second of audio.
pub struct Spectrum {
    pub frames: Vec<[f32; BANDS]>,
}

impl Spectrum {
    /// Levels at `t` seconds, interpolated; wraps around for songs longer than the clip.
    pub fn at(&self, t: f32) -> [f32; BANDS] {
        let n = self.frames.len();
        if n == 0 {
            return [0.; BANDS];
        }
        let pos = (t.max(0.) * FPS) % n as f32;
        let (i, frac) = (pos as usize % n, pos.fract());
        let (a, b) = (&self.frames[i], &self.frames[(i + 1) % n]);
        let mut out = [0.; BANDS];
        for k in 0..BANDS {
            out[k] = a[k] + (b[k] - a[k]) * frac;
        }
        out
    }
}

pub fn analyze(path: &Path) -> Result<Spectrum> {
    let decoder = Decoder::try_from(std::fs::File::open(path)?)?;
    let channels = decoder.channels().get() as usize;
    let rate = decoder.sample_rate().get() as f32;
    let interleaved: Vec<f32> = decoder.collect();
    if interleaved.len() < FFT * channels {
        return Err(anyhow!("clip too short"));
    }
    let mono: Vec<f32> = interleaved.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect();

    // Log-spaced band edges from 40 Hz to 16 kHz, as FFT bin indices.
    let bin_hz = rate / FFT as f32;
    let edges: Vec<usize> = (0..=BANDS)
        .map(|i| {
            let hz = 40. * (16_000f32 / 40.).powf(i as f32 / BANDS as f32);
            ((hz / bin_hz) as usize).clamp(1, FFT / 2 - 1)
        })
        .collect();
    let window: Vec<f32> = (0..FFT).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos()).collect();

    let hop = (rate / FPS) as usize;
    let mut frames = Vec::new();
    let mut re = vec![0f32; FFT];
    let mut im = vec![0f32; FFT];
    let mut start = 0;
    while start + FFT <= mono.len() {
        for i in 0..FFT {
            re[i] = mono[start + i] * window[i];
            im[i] = 0.;
        }
        fft(&mut re, &mut im);
        let mut bands = [0f32; BANDS];
        for b in 0..BANDS {
            let (lo, hi) = (edges[b], edges[b + 1].max(edges[b] + 1));
            let power: f32 = (lo..hi).map(|k| re[k] * re[k] + im[k] * im[k]).sum::<f32>() / (hi - lo) as f32;
            bands[b] = 10. * (power + 1e-12).log10();
        }
        frames.push(bands);
        start += hop;
    }

    // Normalise to 0–1 against the clip's own loudness, with a tilt so highs (naturally
    // quieter) still move, then light temporal smoothing so bars don't flicker.
    let all: Vec<f32> = frames.iter().flat_map(|f| f.iter().copied()).collect();
    let mut sorted = all.clone();
    sorted.sort_by(f32::total_cmp);
    let (floor, peak) = (sorted[sorted.len() / 10], sorted[sorted.len() * 995 / 1000]);
    let range = (peak - floor).max(1.);
    let mut prev = [0f32; BANDS];
    for f in &mut frames {
        for (b, v) in f.iter_mut().enumerate() {
            let tilt = 9. * b as f32 / BANDS as f32;
            let level = ((*v + tilt - floor) / range).clamp(0., 1.).powf(1.4);
            // Fast attack, slower release, like a real meter.
            *v = if level > prev[b] { level } else { prev[b] * 0.82 + level * 0.18 };
            prev[b] = *v;
        }
    }
    Ok(Spectrum { frames })
}

/// In-place iterative radix-2 FFT.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (angle * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_a_pure_tone() {
        let n = 1024;
        let mut re: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * 64. * i as f32 / n as f32).sin()).collect();
        let mut im = vec![0.; n];
        fft(&mut re, &mut im);
        let mags: Vec<f32> = (0..n / 2).map(|k| re[k].hypot(im[k])).collect();
        let peak = mags.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(peak, 64);
    }

    #[test]
    #[ignore]
    fn analyzes_real_previews_differently() {
        let (_, tracks) = crate::api::Catalog::Public.album("6822944288").unwrap();
        let mut means = Vec::new();
        for t in tracks.iter().take(2) {
            let path = crate::api::download_cached(t.preview_url.as_ref().unwrap(), "previews", "m4a").unwrap();
            let started = std::time::Instant::now();
            let s = analyze(&path).unwrap();
            let low: f32 = s.frames.iter().map(|f| f[..16].iter().sum::<f32>()).sum::<f32>() / s.frames.len() as f32;
            let high: f32 = s.frames.iter().map(|f| f[48..].iter().sum::<f32>()).sum::<f32>() / s.frames.len() as f32;
            println!("{}: {} frames in {:?}, low {low:.2} high {high:.2}", t.title, s.frames.len(), started.elapsed());
            means.push((low, high));
        }
        assert!(means[0] != means[1]);
    }
}
