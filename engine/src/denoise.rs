// RNNoise (nnnoiseless) before transcription. Runs at 48 kHz, so 16 kHz input is resampled 3x.
// Some dry signal is mixed back in; full-strength removal blurs soft consonants.
use nnnoiseless::DenoiseState;

const DRY: f32 = 0.15;

struct Wav {
    rate: u32,
    data: std::ops::Range<usize>,
}

fn parse(wav: &[u8]) -> Option<Wav> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([wav[i], wav[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([wav[i], wav[i + 1], wav[i + 2], wav[i + 3]]);
    let (mut pos, mut rate) = (12, None);
    while pos + 8 <= wav.len() {
        let size = u32_at(pos + 4) as usize;
        let body = pos + 8;
        match &wav[pos..pos + 4] {
            b"fmt " if body + 16 <= wav.len() => {
                let (format, channels, bits) = (u16_at(body), u16_at(body + 2), u16_at(body + 14));
                if format != 1 || channels != 1 || bits != 16 {
                    return None;
                }
                rate = Some(u32_at(body + 4));
            }
            b"data" => {
                let end = (body + size).min(wav.len()) & !1;
                return Some(Wav { rate: rate?, data: body..end });
            }
            _ => {}
        }
        pos = body + size + (size & 1);
    }
    None
}

fn clean_48k(input: &[f32]) -> Vec<f32> {
    let mut state = DenoiseState::new();
    let mut out = Vec::with_capacity(input.len() + DenoiseState::FRAME_SIZE);
    let mut frame_out = [0.0f32; DenoiseState::FRAME_SIZE];
    for chunk in input.chunks(DenoiseState::FRAME_SIZE) {
        let mut frame = [0.0f32; DenoiseState::FRAME_SIZE];
        frame[..chunk.len()].copy_from_slice(chunk);
        state.process_frame(&mut frame_out, &frame);
        out.extend_from_slice(&frame_out[..chunk.len()]);
    }
    // first frame is warm-up: fade in from the dry signal
    for (i, s) in out.iter_mut().take(DenoiseState::FRAME_SIZE).enumerate() {
        let t = i as f32 / DenoiseState::FRAME_SIZE as f32;
        *s = *s * t + input[i] * (1.0 - t);
    }
    out
}

/// Unsupported formats are returned unchanged.
pub fn reduce_noise(wav: &[u8]) -> Vec<u8> {
    let Some(Wav { rate, data }) = parse(wav) else { return wav.to_vec() };
    let step = match rate {
        48_000 => 1,
        16_000 => 3,
        _ => return wav.to_vec(),
    };
    // RNNoise wants i16 scale, not -1..1
    let samples: Vec<f32> =
        wav[data.clone()].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32).collect();
    if samples.is_empty() {
        return wav.to_vec();
    }
    let upsampled: Vec<f32> = (0..samples.len() * step)
        .map(|i| {
            let a = samples[i / step];
            let b = *samples.get(i / step + 1).unwrap_or(&a);
            a + (b - a) * (i % step) as f32 / step as f32
        })
        .collect();
    let cleaned = clean_48k(&upsampled);

    let mut out = wav.to_vec();
    for (i, dry) in samples.iter().enumerate() {
        let wet = cleaned[i * step..(i + 1) * step].iter().sum::<f32>() / step as f32;
        let mixed = (wet * (1.0 - DRY) + dry * DRY).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        out[data.start + i * 2..data.start + i * 2 + 2].copy_from_slice(&mixed.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(samples: &[i16]) -> Vec<u8> {
        let mut w = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&[1, 0, 1, 0]);
        w.extend_from_slice(&16_000u32.to_le_bytes());
        w.extend_from_slice(&32_000u32.to_le_bytes());
        w.extend_from_slice(&[2, 0, 16, 0]);
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        for s in samples {
            w.extend_from_slice(&s.to_le_bytes());
        }
        w
    }

    #[test]
    fn steady_hiss_gets_quieter_and_the_header_is_kept() {
        let mut seed = 7u32;
        let hiss: Vec<i16> = (0..16_000)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (seed >> 16) as i16 % 600
            })
            .collect();
        let before = wav(&hiss);
        let after = reduce_noise(&before);
        assert_eq!(after.len(), before.len());
        assert_eq!(after[..44], before[..44]);
        let energy = |w: &[u8]| {
            w[44 + 3200..].chunks_exact(2).map(|b| (i16::from_le_bytes([b[0], b[1]]) as f64).powi(2)).sum::<f64>()
        };
        assert!(energy(&after) < energy(&before) * 0.5);
    }

    #[test]
    fn unknown_formats_pass_through() {
        assert_eq!(reduce_noise(b"not a wav"), b"not a wav".to_vec());
    }
}
