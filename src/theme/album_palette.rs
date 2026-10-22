//! Accent extraction from album artwork: the most prominent vivid hue wins. Runs off the GTK
//! thread on pixels downloaded from the cached texture.

use super::contrast::Rgb;

const BUCKETS: usize = 36;

/// `pixels` is tightly packed RGBA8 or RGB8 (`channels` = 4 or 3), any row stride.
pub fn dominant_accent(pixels: &[u8], width: usize, height: usize, stride: usize, channels: usize) -> Option<Rgb> {
    if channels < 3 || width == 0 || height == 0 {
        return None;
    }
    let mut weight = [0f64; BUCKETS];
    let mut sum = [(0f64, 0f64, 0f64); BUCKETS];
    // ~4k samples regardless of image size.
    let step = ((width * height) as f64 / 4096.0).sqrt().max(1.0) as usize;
    for y in (0..height).step_by(step) {
        for x in (0..width).step_by(step) {
            let i = y * stride + x * channels;
            let Some(px) = pixels.get(i..i + channels) else { continue };
            if channels == 4 && px[3] < 128 {
                continue;
            }
            let c = Rgb::from_u8(px[0], px[1], px[2]);
            let (h, s, l) = c.to_hsl();
            // Vivid mid-tones count most; greys, near-black and near-white barely count.
            let w = s * (1.0 - (2.0 * l - 1.0).abs());
            if w < 0.08 {
                continue;
            }
            let b = ((h / 360.0) * BUCKETS as f64) as usize % BUCKETS;
            weight[b] += w;
            sum[b].0 += c.r * w;
            sum[b].1 += c.g * w;
            sum[b].2 += c.b * w;
        }
    }
    // Neighbouring buckets belong to the same hue family.
    let score = |b: usize| weight[b] + 0.5 * (weight[(b + 1) % BUCKETS] + weight[(b + BUCKETS - 1) % BUCKETS]);
    let best = (0..BUCKETS).max_by(|&a, &b| score(a).total_cmp(&score(b)))?;
    let total: f64 = weight.iter().sum();
    if weight[best] <= 0.0 || total <= 0.0 {
        return None;
    }
    let w = weight[best];
    let avg = Rgb::new(sum[best].0 / w, sum[best].1 / w, sum[best].2 / w);
    // Give washed-out covers a usable accent.
    let (h, s, l) = avg.to_hsl();
    Some(Rgb::from_hsl(h, s.max(0.45), l.clamp(0.35, 0.7)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(colors: &[(u8, u8, u8, usize)]) -> (Vec<u8>, usize, usize) {
        let mut px = Vec::new();
        for &(r, g, b, n) in colors {
            for _ in 0..n {
                px.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let w = 64;
        let h = px.len() / 4 / w;
        (px, w, h)
    }

    #[test]
    fn picks_vivid_majority_over_grey() {
        let (px, w, h) = image(&[(128, 128, 128, 64 * 40), (220, 40, 40, 64 * 16), (40, 80, 220, 64 * 8)]);
        let c = dominant_accent(&px, w, h, w * 4, 4).unwrap();
        let (hue, _, _) = c.to_hsl();
        assert!(!(15.0..345.0).contains(&hue), "expected red, got {c} ({hue})");
    }

    #[test]
    fn greyscale_has_no_accent() {
        let (px, w, h) = image(&[(10, 10, 10, 64 * 32), (240, 240, 240, 64 * 32)]);
        assert!(dominant_accent(&px, w, h, w * 4, 4).is_none());
    }
}
