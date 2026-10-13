//! Frozen v49 renderer for byte-for-byte regression tests.
#![allow(non_snake_case)]
const SCREEN_W: usize = 400;
const SCREEN_H: usize = 240;
const LCD_W: usize = 128;
const LCD_H: usize = 128;
const LCD_X: usize = 136;
const LCD_Y: usize = 106;
const PATCH_PAD: usize = 3;
const PATCH_W: usize = LCD_W + 2 * PATCH_PAD;
const PATCH_H: usize = LCD_H + 2 * PATCH_PAD;
fn put_pixel(buf: &mut [u8], x: usize, y: usize, rgb: [u8; 3]) {
    if x >= SCREEN_W || y >= SCREEN_H {
        return;
    }
    let dst = (x * SCREEN_H + (SCREEN_H - 1 - y)) * 3;
    if dst + 2 < buf.len() {
        buf[dst] = rgb[2];
        buf[dst + 1] = rgb[1];
        buf[dst + 2] = rgb[0];
    }
}

pub fn render(buf: &mut [u8], pixels: &[u8], SHELL_LCD: &[u8]) {
    for y in 0..LCD_H {
        for x in 0..LCD_W {
            let p = (y * LCD_W + x) * 3;
            let q = (y * LCD_W + x) * 4;
            let a = SHELL_LCD[q + 3] as u16;
            let inv = 255 - a;
            let rgb = [
                ((pixels[p] as u16 * inv + SHELL_LCD[q] as u16 * a + 127) / 255) as u8,
                ((pixels[p + 1] as u16 * inv + SHELL_LCD[q + 1] as u16 * a + 127) / 255) as u8,
                ((pixels[p + 2] as u16 * inv + SHELL_LCD[q + 2] as u16 * a + 127) / 255) as u8,
            ];
            put_pixel(buf, LCD_X + x, LCD_Y + y, rgb);
        }
    }
}

/// In stereo mode, redraw the LCD with 3 extra pixels on each side.
/// These repeat the nearest LCD edge pixel only beneath the shell bezel, so
/// shifting the foreground shell +/-1 never exposes a thin galaxy seam.
/// LCD image is NOT filtered or scaled; it stays native 128x128.
pub fn render_stereo_lcd(buf: &mut [u8], pixels: &[u8], shell: &[u8]) {
    for y in 0..PATCH_H {
        for x in 0..PATCH_W {
            let sx = x.saturating_sub(PATCH_PAD).min(LCD_W - 1);
            let sy = y.saturating_sub(PATCH_PAD).min(LCD_H - 1);
            let p = (sy * LCD_W + sx) * 3;
            let q = (y * PATCH_W + x) * 4;
            let a = shell[q + 3] as u16;
            let inv = 255 - a;
            let rgb = [
                ((pixels[p] as u16 * inv + shell[q] as u16 * a + 127) / 255) as u8,
                ((pixels[p + 1] as u16 * inv + shell[q + 1] as u16 * a + 127) / 255) as u8,
                ((pixels[p + 2] as u16 * inv + shell[q + 2] as u16 * a + 127) / 255) as u8,
            ];
            put_pixel(buf, LCD_X + x - PATCH_PAD, LCD_Y + y - PATCH_PAD, rgb);
        }
    }
}
