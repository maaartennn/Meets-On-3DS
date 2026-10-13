//! Host tests import the production modules without linking the 3DS services.
#![allow(dead_code)]
// rustc-only QA imports the exact supplied core sound module. Cargo tests use
// the real sibling tg18-core dependency instead.
#[cfg(feature = "standalone-core-sound")]
extern crate self as tg18;
#[path = "../src/audio.rs"]
mod audio;
mod audio_tests;
#[path = "../src/diagnostics.rs"]
mod diagnostics;
#[path = "../src/lcd.rs"]
mod lcd;
mod legacy_audio;
#[path = "../src/photo.rs"]
mod photo;
#[path = "reference_renderer.rs"]
mod reference_renderer;
#[cfg(feature = "standalone-core-sound")]
#[path = "../../tg18-emu-main/core/src/sound.rs"]
pub mod sound;

// Profiler tests do not draw; graphics service wiring is checked by a 3DS build.
fn blend_rgba_bottom(_: &mut [u8], _: usize, _: usize, _: usize, _: usize, _: &[u8]) {}

const FLAT: &[u8] = include_bytes!("../assets/runtime/shell_lcd.rgba");
const LEFT: &[u8] = include_bytes!("../assets/runtime/shell_lcd_left.rgba");
const RIGHT: &[u8] = include_bytes!("../assets/runtime/shell_lcd_right.rgba");

fn compare_renderers(pixels: &[u8], shell: &[u8], stereo: bool) {
    let mut original = (0..400 * 240 * 3)
        .map(|i| (i * 17) as u8)
        .collect::<Vec<_>>();
    let mut updated = original.clone();
    if stereo {
        reference_renderer::render_stereo_lcd(&mut original, pixels, shell);
        lcd::render_stereo(&mut updated, pixels, shell);
    } else {
        reference_renderer::render(&mut original, pixels, shell);
        lcd::render(&mut updated, pixels, shell);
    }
    assert_eq!(
        updated, original,
        "LCD, bezel, and untouched background must match v49 exactly"
    );
}

#[test]
fn real_shell_assets_match_original_rendering() {
    for pattern in 0..8 {
        let pixels = (0..128 * 128 * 3)
            .map(|i| (i * (17 + pattern) + pattern * 31) as u8)
            .collect::<Vec<_>>();
        compare_renderers(&pixels, FLAT, false);
        compare_renderers(&pixels, LEFT, true);
        compare_renderers(&pixels, RIGHT, true);
    }
}

#[test]
fn all_alpha_values_match_original_rounding() {
    let pixels = (0..128 * 128 * 3)
        .map(|i| (i * 73) as u8)
        .collect::<Vec<_>>();
    let shell = (0..128 * 128 * 4)
        .map(|i| {
            if i % 4 == 3 {
                (i / 4) as u8
            } else {
                (i * 113) as u8
            }
        })
        .collect::<Vec<_>>();
    compare_renderers(&pixels, &shell, false);
}
