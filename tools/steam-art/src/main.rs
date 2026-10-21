//! Draws the Steam artwork of Midnight Club: Los Angeles: the portrait and wide
//! capsules, the hero, the logo and the icon. The pictures come from
//! screenshots the game took of itself on this machine from the player's own
//! data (scripts/artwork.sh), so nothing of the game is shipped; without any
//! screenshot a night-blue gradient stands in.
//!
//!     steam-art OUT [SHOT.png ...]
//!
//! The first readable screenshot is used for the hero and the wide capsule,
//! the second (or the first again) for the portrait capsule and the icon.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};

const FONT: &[u8] = include_bytes!("../fonts/Anton-Regular.ttf");
const TITLE: &str = "MIDNIGHT CLUB";
const SUBTITLE: &str = "LOS ANGELES";
const ACCENT: [u8; 3] = [255, 72, 56];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(out) = args.first().map(PathBuf::from) else {
        eprintln!("usage: steam-art OUT [SHOT.png ...]");
        std::process::exit(2);
    };
    let shots: Vec<RgbaImage> = args[1..]
        .iter()
        .filter_map(|p| match image::open(p) {
            Ok(i) if i.width() >= 320 && i.height() >= 180 && !blank(&i.to_rgba8()) => Some(i.to_rgba8()),
            Ok(_) => {
                eprintln!("{p}: too small or blank, left out");
                None
            }
            Err(e) => {
                eprintln!("{p}: {e}");
                None
            }
        })
        .collect();
    let font = FontRef::try_from_slice(FONT).expect("the font");
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("error: {}: {e}", out.display());
        std::process::exit(1);
    }
    let wide_src = shots.first();
    let tall_src = shots.get(1).or(shots.first());
    let pictures: [(&str, RgbaImage); 5] = [
        ("hero.png", hero(wide_src, 1920, 620)),
        ("wide.png", titled(wide_src, 920, 430, &font)),
        ("capsule.png", titled(tall_src, 600, 900, &font)),
        ("logo.png", logo(&font)),
        ("icon.png", icon(tall_src, &font)),
    ];
    for (name, img) in pictures {
        let path = out.join(name);
        if let Err(e) = save(&img, &path) {
            eprintln!("error: {}: {e}", path.display());
            std::process::exit(1);
        }
    }
    println!(
        "artwork in {} ({})",
        out.display(),
        if shots.is_empty() { "no screenshot: drawn plain".to_string() } else { format!("from {} screenshot(s)", shots.len()) }
    );
}

fn save(img: &RgbaImage, path: &Path) -> image::ImageResult<()> {
    let tmp = path.with_extension("png.tmp");
    img.save_with_format(&tmp, image::ImageFormat::Png)?;
    std::fs::rename(&tmp, path).map_err(image::ImageError::IoError)
}

/// A frame that is (nearly) one colour: a screenshot of a window that showed nothing.
fn blank(img: &RgbaImage) -> bool {
    let (mut lo, mut hi) = (255u8, 0u8);
    for p in img.pixels().step_by(97) {
        let l = luma(p);
        lo = lo.min(l);
        hi = hi.max(l);
    }
    hi.saturating_sub(lo) < 12
}

fn luma(p: &Rgba<u8>) -> u8 {
    ((p[0] as u32 * 54 + p[1] as u32 * 183 + p[2] as u32 * 19) >> 8) as u8
}

/// The picture scaled to cover w x h and cut to it around (fx, fy) (0..1).
fn cover(src: &RgbaImage, w: u32, h: u32, fx: f32, fy: f32) -> RgbaImage {
    let scale = (w as f32 / src.width() as f32).max(h as f32 / src.height() as f32);
    let sw = ((src.width() as f32 * scale).ceil() as u32).max(w);
    let sh = ((src.height() as f32 * scale).ceil() as u32).max(h);
    let scaled = imageops::resize(src, sw, sh, FilterType::Lanczos3);
    let x = ((sw - w) as f32 * fx) as u32;
    let y = ((sh - h) as f32 * fy) as u32;
    imageops::crop_imm(&scaled, x, y, w, h).to_image()
}

/// The background: the screenshot, or a night sky gradient.
fn background(src: Option<&RgbaImage>, w: u32, h: u32) -> RgbaImage {
    match src {
        Some(s) => cover(s, w, h, 0.5, 0.55),
        None => RgbaImage::from_fn(w, h, |_, y| {
            let t = y as f32 / h as f32;
            let mix = |a: f32, b: f32| (a + (b - a) * t) as u8;
            Rgba([mix(10.0, 48.0), mix(14.0, 22.0), mix(40.0, 70.0), 255])
        }),
    }
}

/// Darkens towards the bottom (from `from`, 0..1 of the height) so text reads over any picture.
fn shade_bottom(img: &mut RgbaImage, from: f32, strength: f32) {
    let h = img.height() as f32;
    for (_, y, p) in img.enumerate_pixels_mut() {
        let t = ((y as f32 / h - from) / (1.0 - from)).clamp(0.0, 1.0);
        let k = 1.0 - strength * t * t.sqrt();
        for c in 0..3 {
            p[c] = (p[c] as f32 * k) as u8;
        }
    }
}

fn hero(src: Option<&RgbaImage>, w: u32, h: u32) -> RgbaImage {
    let mut img = background(src, w, h);
    shade_bottom(&mut img, 0.6, 0.35);
    img
}

/// Width of `text` at `size` pixels.
fn text_width(font: &FontRef, size: f32, text: &str) -> f32 {
    let f = font.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = prev {
            w += f.kern(p, id);
        }
        w += f.h_advance(id);
        prev = Some(id);
    }
    w
}

/// Draws `text` with its baseline at (x, y), blending `color` over the picture.
fn draw_text(img: &mut RgbaImage, font: &FontRef, size: f32, x: f32, y: f32, text: &str, color: [u8; 3]) {
    let f = font.as_scaled(PxScale::from(size));
    let mut pen = x;
    let mut prev = None;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = prev {
            pen += f.kern(p, id);
        }
        let glyph = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(pen, y));
        pen += f.h_advance(id);
        prev = Some(id);
        let Some(outline) = font.outline_glyph(glyph) else { continue };
        let b = outline.px_bounds();
        outline.draw(|gx, gy, cov| {
            let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
            if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                return;
            }
            let p = img.get_pixel_mut(px as u32, py as u32);
            let a = cov.clamp(0.0, 1.0);
            for c in 0..3 {
                p[c] = (p[c] as f32 * (1.0 - a) + color[c] as f32 * a) as u8;
            }
            p[3] = p[3].max((a * 255.0) as u8);
        });
    }
}

/// The title, centred, its bottom line at `bottom`, `width` wide at most.
fn title_block(img: &mut RgbaImage, font: &FontRef, width: f32, bottom: f32) {
    let cx = img.width() as f32 / 2.0;
    let big = (width / text_width(font, 100.0, TITLE) * 100.0).min(img.height() as f32 * 0.2);
    let small = big * 0.42;
    let sub_w = text_width(font, small, SUBTITLE);
    // (a soft shadow first, then the letters)
    for (dx, dy, col) in [(3.0, 3.0, [0, 0, 0]), (0.0, 0.0, [255, 255, 255])] {
        draw_text(img, font, big, cx - text_width(font, big, TITLE) / 2.0 + dx, bottom - small * 1.25 + dy, TITLE, col);
    }
    for (dx, dy, col) in [(2.0, 2.0, [0, 0, 0]), (0.0, 0.0, ACCENT)] {
        draw_text(img, font, small, cx - sub_w / 2.0 + dx, bottom + dy, SUBTITLE, col);
    }
}

fn titled(src: Option<&RgbaImage>, w: u32, h: u32, font: &FontRef) -> RgbaImage {
    let mut img = background(src, w, h);
    shade_bottom(&mut img, 0.35, 0.8);
    title_block(&mut img, font, w as f32 * 0.84, h as f32 * 0.9);
    img
}

/// The title alone on a transparent picture, which Steam lays over the hero.
fn logo(font: &FontRef) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(1280, 400, Rgba([0, 0, 0, 0]));
    // (on a transparent picture the shadow pass sets the alpha the letters then keep)
    title_block(&mut img, font, 1180.0, 360.0);
    img
}

fn icon(src: Option<&RgbaImage>, font: &FontRef) -> RgbaImage {
    let mut img = background(src, 256, 256);
    shade_bottom(&mut img, 0.0, 0.55);
    let size = 150.0;
    let w = text_width(font, size, "MC");
    draw_text(&mut img, font, size, 128.0 - w / 2.0 + 3.0, 190.0 + 3.0, "MC", [0, 0, 0]);
    draw_text(&mut img, font, size, 128.0 - w / 2.0, 190.0, "MC", [255, 255, 255]);
    let s = 46.0;
    let w2 = text_width(font, s, "LA");
    draw_text(&mut img, font, s, 128.0 - w2 / 2.0, 238.0, "LA", ACCENT);
    img
}
