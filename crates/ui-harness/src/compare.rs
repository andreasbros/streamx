//! Screenshot comparison with tolerance, so the same baselines can serve
//! Linux and macOS captures of the identical GPUI UI (font rasterization
//! differs slightly between platforms).

use std::path::Path;

use anyhow::{Context, Result};

/// Mean absolute per-channel difference in [0, 1]. Images of different
/// dimensions are scaled to the smaller common size before comparing.
pub fn diff_ratio(a: &Path, b: &Path) -> Result<f64> {
    let img_a = image::open(a).with_context(|| format!("open {}", a.display()))?;
    let img_b = image::open(b).with_context(|| format!("open {}", b.display()))?;
    let w = img_a.width().min(img_b.width());
    let h = img_a.height().min(img_b.height());
    let a = image::imageops::resize(
        &img_a.to_rgb8(),
        w,
        h,
        image::imageops::FilterType::Triangle,
    );
    let b = image::imageops::resize(
        &img_b.to_rgb8(),
        w,
        h,
        image::imageops::FilterType::Triangle,
    );
    let mut total: u64 = 0;
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        for c in 0..3 {
            total += (pa.0[c] as i32 - pb.0[c] as i32).unsigned_abs() as u64;
        }
    }
    let denom = (w as u64) * (h as u64) * 3 * 255;
    Ok(total as f64 / denom as f64)
}

/// How visually "busy" an image is: standard deviation of luminance in
/// [0, 1]. A window of skeleton placeholders scores low; a grid of real
/// posters scores high. Used to verify posters actually rendered.
pub fn colorfulness(path: &Path) -> Result<f64> {
    region_colorfulness(path, 0.0, 1.0)
}

/// `colorfulness` over a horizontal band of the image (fractions of the
/// height). Lets the harness assert e.g. that the bottom of the movie
/// page shows the backdrop image rather than flat app background.
pub fn region_colorfulness(path: &Path, from_frac: f64, to_frac: f64) -> Result<f64> {
    let img = image::open(path)?.to_luma8();
    let h = img.height() as f64;
    let y0 = ((h * from_frac) as u32).min(img.height().saturating_sub(1));
    let y1 = ((h * to_frac) as u32).clamp(y0 + 1, img.height());
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for y in y0..y1 {
        for x in 0..img.width() {
            sum += img.get_pixel(x, y).0[0] as f64;
            n += 1;
        }
    }
    let n = n.max(1);
    let mean = sum / n as f64;
    let mut var = 0.0f64;
    for y in y0..y1 {
        for x in 0..img.width() {
            let d = img.get_pixel(x, y).0[0] as f64 - mean;
            var += d * d;
        }
    }
    Ok((var / n as f64).sqrt() / 255.0)
}
