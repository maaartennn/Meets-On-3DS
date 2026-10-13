//! Desktop comparison only. This does not predict timing on ARM / Nintendo 3DS.
#[path = "../src/lcd.rs"]
mod lcd;
#[path = "reference_renderer.rs"]
mod reference_renderer;
use std::hint::black_box;
use std::time::Instant;

fn measure(render: impl Fn(&mut [u8], &[u8], &[u8]), shell: &[u8]) -> u128 {
    let mut frame = vec![0u8; 400 * 240 * 3];
    let pixels = (0..128 * 128 * 3)
        .map(|i| (i * 73) as u8)
        .collect::<Vec<_>>();
    let start = Instant::now();
    for _ in 0..1000 {
        render(black_box(&mut frame), black_box(&pixels), black_box(shell));
        black_box(&frame);
    }
    start.elapsed().as_micros()
}

fn main() {
    let flat = include_bytes!("../assets/runtime/shell_lcd.rgba");
    let left = include_bytes!("../assets/runtime/shell_lcd_left.rgba");
    let right = include_bytes!("../assets/runtime/shell_lcd_right.rgba");
    println!("1000 draws, microseconds (desktop, not 3DS):");
    println!(
        "flat: old={} new={}",
        measure(reference_renderer::render, flat),
        measure(lcd::render, flat)
    );
    println!(
        "left: old={} new={}",
        measure(reference_renderer::render_stereo_lcd, left),
        measure(lcd::render_stereo, left)
    );
    println!(
        "right: old={} new={}",
        measure(reference_renderer::render_stereo_lcd, right),
        measure(lcd::render_stereo, right)
    );
}
