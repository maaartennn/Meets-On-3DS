//! Native RGB LCD compositing into the 3DS column-major BGR framebuffer.

const SCREEN_W: usize = 400;
const SCREEN_H: usize = 240;
const LCD_W: usize = 128;
const LCD_H: usize = 128;
const LCD_X: usize = 136;
const LCD_Y: usize = 106;
const PATCH_PAD: usize = 3;
const PATCH_W: usize = LCD_W + 2 * PATCH_PAD;
const PATCH_H: usize = LCD_H + 2 * PATCH_PAD;

#[inline]
fn composite(dst: &mut [u8], rgb: &[u8], shell: &[u8]) {
    // Most LCD pixels have no shell overlay. Preserve their exact RGB value
    // without doing three alpha multiplications and divisions per pixel.
    match shell[3] {
        0 => dst.copy_from_slice(&[rgb[2], rgb[1], rgb[0]]),
        255 => dst.copy_from_slice(&[shell[2], shell[1], shell[0]]),
        alpha => {
            let a = alpha as u16;
            let inv = 255 - a;
            for ch in 0..3 {
                dst[2 - ch] = ((rgb[ch] as u16 * inv + shell[ch] as u16 * a + 127) / 255) as u8;
            }
        }
    }
}

pub fn render(buf: &mut [u8], pixels: &[u8], shell: &[u8]) {
    debug_assert_eq!(buf.len(), SCREEN_W * SCREEN_H * 3);
    debug_assert_eq!(pixels.len(), LCD_W * LCD_H * 3);
    debug_assert_eq!(shell.len(), LCD_W * LCD_H * 4);
    // Visit columns first to keep destination writes adjacent in memory.
    for x in 0..LCD_W {
        for y in 0..LCD_H {
            let src = y * LCD_W + x;
            let dst = ((LCD_X + x) * SCREEN_H + SCREEN_H - 1 - LCD_Y - y) * 3;
            composite(
                &mut buf[dst..dst + 3],
                &pixels[src * 3..src * 3 + 3],
                &shell[src * 4..src * 4 + 4],
            );
        }
    }
}

/// Repeat the LCD's edge pixels under the stereo bezel to avoid galaxy seams.
pub fn render_stereo(buf: &mut [u8], pixels: &[u8], shell: &[u8]) {
    debug_assert_eq!(buf.len(), SCREEN_W * SCREEN_H * 3);
    debug_assert_eq!(pixels.len(), LCD_W * LCD_H * 3);
    debug_assert_eq!(shell.len(), PATCH_W * PATCH_H * 4);
    for x in 0..PATCH_W {
        let sx = x.saturating_sub(PATCH_PAD).min(LCD_W - 1);
        for y in 0..PATCH_H {
            let sy = y.saturating_sub(PATCH_PAD).min(LCD_H - 1);
            let src = sy * LCD_W + sx;
            let overlay = (y * PATCH_W + x) * 4;
            let dst =
                ((LCD_X + x - PATCH_PAD) * SCREEN_H + SCREEN_H - 1 - (LCD_Y + y - PATCH_PAD)) * 3;
            composite(
                &mut buf[dst..dst + 3],
                &pixels[src * 3..src * 3 + 3],
                &shell[overlay..overlay + 4],
            );
        }
    }
}
