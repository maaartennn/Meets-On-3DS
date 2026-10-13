//! Boot-only firmware selector. Does not change the emulator or stereo rendering.
use ctru::prelude::*;
use ctru::services::gfx::{Flush, Screen};
use std::fs;

const ROOT: &str = "sdmc:/3ds/tg18-3ds";
const FIRMWARE_DIR: &str = "sdmc:/3ds/tg18-3ds/firmware";
const LEGACY_WONDER_GARDEN: &str = "sdmc:/3ds/tg18-3ds/firmware.bin";
const IMAGE_SIZE: u64 = 8 * 1024 * 1024;

// The nine ROMs in the user's supported firmware collection. The UI scans
// for these filenames on the SD, and displays only the ones actually found.
const FILES: [&str; 9] = [
    "fw_tg18_en_v063_2020-02-13_wondergarden.bin",
    "fw_tg18_en_v058_2019-05-13_magic.bin",
    "fw_tg18_en_v057_2019-05-13_fairy.bin",
    "fw_tg18_jp_v031_2018-10-19_magic.bin",
    "fw_tg18_jp_v030_2018-10-19_fairy.bin",
    "fw_tg18_jp_v055_2019-03-10_pastel.bin",
    "fw_tg18_jp_v047_2019-05-28_fantasy.bin",
    "fw_tg18_jp_v062_2019-09-12_sweets.bin",
    "fw_tg18_jp_v056_2019-04-24_sanrio.bin",
];
const SAVE_NAMES: [&str; 9] = [
    "on-wonder-garden",
    "on-magic",
    "on-fairy",
    "meets-magic",
    "meets-fairy",
    "meets-pastel",
    "meets-fantasy",
    "meets-sweets",
    "meets-sanrio",
];

pub struct FirmwareChoice {
    pub id: usize,
    pub path: String,
}

pub fn discover() -> Vec<FirmwareChoice> {
    let mut found = Vec::new();
    for (id, name) in FILES.iter().enumerate() {
        let path = format!("{}/{}", FIRMWARE_DIR, name);
        if fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == IMAGE_SIZE) {
            found.push(FirmwareChoice { id, path });
        }
    }
    // Keep the original Wonder Garden installation available even before the
    // user transfers the new ROM collection. Never show it twice.
    if !found.iter().any(|f| f.id == 0)
        && fs::metadata(LEGACY_WONDER_GARDEN).is_ok_and(|m| m.is_file() && m.len() == IMAGE_SIZE)
    {
        found.insert(
            0,
            FirmwareChoice {
                id: 0,
                path: LEGACY_WONDER_GARDEN.to_string(),
            },
        );
    }
    found
}

// v39's existing Wonder Garden save1.bin/save2.bin remain in place. Other
// versions use separate folders so selecting a different ROM cannot overwrite
// the family's progress. Never migrate, rename or delete existing saves.
pub fn save_slots(id: usize) -> Result<[String; 2], String> {
    if id == 0 {
        return Ok([format!("{}/save1.bin", ROOT), format!("{}/save2.bin", ROOT)]);
    }
    let name = SAVE_NAMES.get(id).ok_or("Unknown firmware save path")?;
    let dir = format!("{}/saves/{}", ROOT, name);
    fs::create_dir_all(&dir).map_err(|e| format!("Save folder creation failed: {}", e))?;
    Ok([format!("{}/save1.bin", dir), format!("{}/save2.bin", dir)])
}

const W: usize = 320;
const H: usize = 240;
const ROW_X: usize = 15;
const ROW_Y: usize = 42;
const ROW_STEP: usize = 20;
const ROW_W: usize = 290;
const ROW_H: usize = 19;
const EMPTY_H: usize = 76;
const BG: &[u8; W * H * 3] = include_bytes!("../assets/runtime/firmware_picker_bg.bgr");
const EMPTY: &[u8; ROW_W * EMPTY_H * 4] =
    include_bytes!("../assets/runtime/firmware_picker_empty.rgba");
const ROWS: [[&[u8; ROW_W * ROW_H * 4]; 2]; 9] = [
    [
        include_bytes!("../assets/runtime/firmware_row_0_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_0_1.rgba"),
    ],
    [
        include_bytes!("../assets/runtime/firmware_row_1_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_1_1.rgba"),
    ],
    [
        include_bytes!("../assets/runtime/firmware_row_2_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_2_1.rgba"),
    ],
    [
        include_bytes!("../assets/runtime/firmware_row_3_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_3_1.rgba"),
    ],
    [
        include_bytes!("../assets/runtime/firmware_row_4_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_4_1.rgba"),
    ],
    [
        include_bytes!("../assets/runtime/firmware_row_5_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_5_1.rgba"),
    ],
    [
        include_bytes!("../assets/runtime/firmware_row_6_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_6_1.rgba"),
    ],
    [
        include_bytes!("../assets/runtime/firmware_row_7_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_7_1.rgba"),
    ],
    [
        include_bytes!("../assets/runtime/firmware_row_8_0.rgba"),
        include_bytes!("../assets/runtime/firmware_row_8_1.rgba"),
    ],
];

// Assets have standard top-to-bottom RGBA order. The framebuffer is BGR8,
// column-major and bottom-up, matching the existing emulator renderer.
fn blend(buf: &mut [u8], x0: usize, y0: usize, w: usize, h: usize, rgba: &[u8]) {
    for y in 0..h {
        for x in 0..w {
            let a = rgba[(y * w + x) * 4 + 3] as u16;
            if a == 0 {
                continue;
            }
            let src = (y * w + x) * 4;
            let dst = ((x0 + x) * H + (H - 1 - (y0 + y))) * 3;
            let inv = 255 - a;
            for ch in 0..3 {
                buf[dst + ch] = ((buf[dst + ch] as u16 * inv + rgba[src + 2 - ch] as u16 * a + 127)
                    / 255) as u8;
            }
        }
    }
}

fn draw(gfx: &Gfx, scratch: &mut [u8], list: &[FirmwareChoice], selected: usize) {
    scratch.copy_from_slice(BG);
    if list.is_empty() {
        blend(scratch, ROW_X, 83, ROW_W, EMPTY_H, EMPTY);
    } else {
        for (row, entry) in list.iter().enumerate() {
            blend(
                scratch,
                ROW_X,
                ROW_Y + row * ROW_STEP,
                ROW_W,
                ROW_H,
                ROWS[entry.id][usize::from(row == selected)],
            );
        }
    }
    let mut bottom = gfx.bottom_screen.borrow_mut();
    let fb = bottom.raw_framebuffer();
    unsafe {
        fb.ptr.copy_from(scratch.as_ptr(), scratch.len());
    }
    bottom.flush_buffers();
}

/// Present the firmware list below the unchanged white-on-black Tamagotchi
/// logo. D-pad up/down and A, or a direct touch, picks a firmware. START exits.
/// No game memory, audio, or save state exists until after this returns.
pub fn select(apt: &Apt, hid: &mut Hid, gfx: &Gfx, list: &[FirmwareChoice]) -> Option<usize> {
    let mut scratch = vec![0u8; W * H * 3];
    let mut selected = 0usize;
    draw(gfx, &mut scratch, list, selected);
    gfx.wait_for_vblank();
    while apt.main_loop() {
        hid.scan_input();
        let down = hid.keys_down();
        if down.contains(KeyPad::START) {
            return None;
        }
        if !list.is_empty() {
            let prev = selected;
            if down.intersects(KeyPad::UP) {
                selected = if selected == 0 {
                    list.len() - 1
                } else {
                    selected - 1
                };
            }
            if down.intersects(KeyPad::DOWN) {
                selected = (selected + 1) % list.len();
            }
            if down.contains(KeyPad::A) {
                return Some(selected);
            }
            if down.contains(KeyPad::TOUCH) {
                let (tx, ty) = hid.touch_position();
                let x = tx as usize;
                let y = ty as usize;
                if x >= ROW_X && x < ROW_X + ROW_W && y >= ROW_Y {
                    let pos = y - ROW_Y;
                    let row = pos / ROW_STEP;
                    if row < list.len() && pos % ROW_STEP < ROW_H {
                        return Some(row);
                    }
                }
            }
            if selected != prev {
                draw(gfx, &mut scratch, list, selected);
            }
        }
        gfx.wait_for_vblank();
    }
    None
}
