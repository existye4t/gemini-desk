/// Programmatically renders a clean, high-DPI anti-aliased RGBA icon for GeminiDesk.
///
/// Uses an independent dark slate rounded-square badge with a crisp four-point star prism
/// in cool cyan-to-indigo gradient so GeminiDesk has a refined, distinct desktop identity
/// without impersonating Google's official trademarked brand assets.
pub fn render_app_icon_rgba(size: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;

    // 4x4 supersampling for razor-sharp anti-aliased curves at 16px, 32px, and 64px.
    const SAMPLES: usize = 4;
    let inv_samples = 1.0 / (SAMPLES * SAMPLES) as f32;

    for y in 0..size {
        for x in 0..size {
            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut acc_a = 0.0f32;

            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let u = ((x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / s) * 2.0 - 1.0;
                    let v = ((y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / s) * 2.0 - 1.0;

                    let (r, g, b, a) = sample_icon(u, v);
                    acc_r += r * a;
                    acc_g += g * a;
                    acc_b += b * a;
                    acc_a += a;
                }
            }

            let out_a = (acc_a * inv_samples).clamp(0.0, 1.0);
            let idx = ((y * size + x) * 4) as usize;
            if out_a > 0.0001 {
                let denom = acc_a.max(0.0001);
                pixels[idx] = ((acc_r / denom).clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels[idx + 1] = ((acc_g / denom).clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels[idx + 2] = ((acc_b / denom).clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels[idx + 3] = (out_a * 255.0).round() as u8;
            }
        }
    }

    pixels
}

/// Signed distance to a rounded square centered at the origin with half-extent `b` and radius `r`.
fn sd_rounded_box(px: f32, py: f32, b: f32, r: f32) -> f32 {
    let qx = px.abs() - (b - r);
    let qy = py.abs() - (b - r);
    let outside = (qx.max(0.0).hypot(qy.max(0.0))) - r;
    let inside = qx.max(qy).min(0.0);
    outside + inside
}

fn sample_icon(u: f32, v: f32) -> (f32, f32, f32, f32) {
    // Outer rounded squircle container
    let d_bg = sd_rounded_box(u, v, 0.92, 0.28);
    if d_bg > 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    // Subtle deep slate radial background (#171b24 -> #101218)
    let rad = (u * u + v * v).sqrt().min(1.2);
    let mut r = 0.11 - 0.03 * rad;
    let mut g = 0.125 - 0.03 * rad;
    let mut b = 0.165 - 0.03 * rad;

    // Subtle rim highlight along inner edge
    if d_bg > -0.045 {
        r += 0.07;
        g += 0.09;
        b += 0.14;
    }

    // Crisp 4-point astroid/sparkle glyph: (|x|^p + |y|^p) <= R^p with p = 0.62
    let star_val = u.abs().powf(0.62) + v.abs().powf(0.62);
    let star_radius = 0.68f32.powf(0.62);
    if star_val <= star_radius {
        // Cool indigo-to-sky-blue diagonal gradient
        let t = ((u - v) * 0.5 + 0.5).clamp(0.0, 1.0);
        let sr = 0.30 + 0.25 * t;
        let sg = 0.56 + 0.30 * t;
        let sb = 0.98;

        // Subtle bright core
        let core = (1.0 - (u.hypot(v) / 0.34)).clamp(0.0, 1.0);
        r = (sr + core * 0.32).min(1.0);
        g = (sg + core * 0.28).min(1.0);
        b = (sb + core * 0.10).min(1.0);
    }

    (r, g, b, 1.0)
}

use std::sync::OnceLock;

static WINDOW_ICON_RGBA: OnceLock<Vec<u8>> = OnceLock::new();
static TRAY_ICON_RGBA: OnceLock<Vec<u8>> = OnceLock::new();

pub fn window_icon() -> Option<tao::window::Icon> {
    let size = 64u32;
    let rgba = WINDOW_ICON_RGBA.get_or_init(|| render_app_icon_rgba(size));
    tao::window::Icon::from_rgba(rgba.clone(), size, size).ok()
}

pub fn tray_icon() -> Option<tray_icon::Icon> {
    let size = 32u32;
    let rgba = TRAY_ICON_RGBA.get_or_init(|| render_app_icon_rgba(size));
    tray_icon::Icon::from_rgba(rgba.clone(), size, size).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expected_rgba_dimensions_and_visible_center() {
        let size = 32u32;
        let rgba = render_app_icon_rgba(size);
        assert_eq!(rgba.len(), (size * size * 4) as usize);

        // Center pixel should be opaque and bright
        let center_idx = (((size / 2) * size + (size / 2)) * 4) as usize;
        assert_eq!(rgba[center_idx + 3], 255);
        assert!(
            rgba[center_idx + 2] > 180,
            "center should have bright blue channel"
        );

        // Extreme corner should be transparent (rounded corners)
        assert_eq!(rgba[3], 0);
    }
}
