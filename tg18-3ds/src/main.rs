//! Tamagotchi Meets/ON frontend: stereo rendering, input, audio, and SD saves.

#![feature(allocator_api)] // needed for the audio buffers in LINEAR memory

use ctru::linear::{LinearAllocation, LinearAllocator};
use ctru::prelude::*;
use ctru::services::gfx::{Flush, Screen, Swap, TopScreen3D};
use ctru::services::ndsp::{
    wave::{Status, Wave},
    AudioFormat, AudioMix, InterpolationType, Ndsp, OutputMode,
};
use std::fs;
use std::sync::Arc;
use std::time::Instant;
use tg18::save;
use tg18::{rtc_seconds_now, Key, Machine, Outcome, CPU_HZ};
mod audio;
mod diagnostics;
mod firmware_picker;
mod lcd;
mod photo;

// libctru function (already linked in by ctru-rs). On an Old 3DS it does nothing.
extern "C" {
    fn osSetSpeedupEnable(enable: bool);
    // libctru's flush writes back the full byte size for BOTH stereo eyes.
    fn gfxFlushBuffers();
    fn DSP_FlushDataCache(address: *const std::ffi::c_void, size: u32) -> i32;
}

// The TOP screen as you hold the console: 400 wide, 240 tall.
// (Its frame buffer is stored in columns: 400 columns of 240 pixels.)
const SCREEN_W: usize = 400;
const SCREEN_H: usize = 240;
const BOTTOM_W: usize = 320;
const BOTTOM_H: usize = 240;

// Assets are preconverted into the 3DS framebuffer's bottom-up, column-major
// BGR8 format. This avoids PNG decoding and file I/O during emulation.
// Editable PNG artwork is kept separately in assets/artwork/.
const TOP_BG: &[u8; SCREEN_W * SCREEN_H * 3] =
    include_bytes!("../assets/runtime/top_with_shell.bgr");
// Boot wordmark: original user artwork recolored white on pure black, in 3DS BGR8.
const BOOT_LOGO: &[u8; SCREEN_W * SCREEN_H * 3] = include_bytes!("../assets/runtime/boot_logo.bgr");
const BOOT_MIN_LOGO_MS: u128 = 650;
const LOGO_FADE_FRAMES: usize = 16;
const SCENE_FADE_FRAMES: usize = 22;
const EXIT_FADE_FRAMES: usize = 20;
// RGBA patch for the region of the shell overlapping the 128x128 LCD.
// This is rendered OVER the LCD to preserve rounded/beveled edges.
const LCD_W: usize = 128;
const LCD_H: usize = 128;
const LCD_X: usize = 136;
const LCD_Y: usize = 106;
const SHELL_LCD: &[u8; LCD_W * LCD_H * 4] = include_bytes!("../assets/runtime/shell_lcd.rgba");
const BOTTOM_BG: &[u8; BOTTOM_W * BOTTOM_H * 3] =
    include_bytes!("../assets/runtime/bottom_with_shell.bgr");
// Each eye has its own precomposed starfield + shell. The LCD is rendered live.
const TOP_BG_LEFT: &[u8; SCREEN_W * SCREEN_H * 3] =
    include_bytes!("../assets/runtime/top_stereo_left.bgr");
const TOP_BG_RIGHT: &[u8; SCREEN_W * SCREEN_H * 3] =
    include_bytes!("../assets/runtime/top_stereo_right.bgr");
const PATCH_PAD: usize = 3;
const PATCH_W: usize = LCD_W + 2 * PATCH_PAD;
const PATCH_H: usize = LCD_H + 2 * PATCH_PAD;
const SHELL_LEFT: &[u8; PATCH_W * PATCH_H * 4] =
    include_bytes!("../assets/runtime/shell_lcd_left.rgba");
const SHELL_RIGHT: &[u8; PATCH_W * PATCH_H * 4] =
    include_bytes!("../assets/runtime/shell_lcd_right.rgba");
const STEREO_SLIDER_THRESHOLD: f32 = 0.03;
// Top-screen notifications are drawn only into the final eye framebuffers.
// They are NEVER written to the pristine scene buffers used for 3D photos.
const TOAST_W: usize = 150;
const TOAST_H: usize = 22;
const TOAST_X: usize = 4;
const TOAST_Y: usize = 0;
const SAVE_NOTICE_MS: u128 = 2000;
const SAVE_NOTICE_EXIT_MS: u128 = 1450;
const TOAST_SAVED: &[u8; TOAST_W * TOAST_H * 4] =
    include_bytes!("../assets/runtime/toast_saved.rgba");
const TOAST_PHOTO: &[u8; TOAST_W * TOAST_H * 4] =
    include_bytes!("../assets/runtime/toast_photo.rgba");
const TOAST_FAILED: &[u8; TOAST_W * TOAST_H * 4] =
    include_bytes!("../assets/runtime/toast_failed.rgba");

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastKind {
    Saved,
    Photo,
    Failed,
}

// v25: pre-rendered backgrounds retain all original artwork and stereoscopy.
// Tiny star masks protect the entire top LCD/shell and bottom shell/planet.
// Bit order is LSB first, indexed by visible y * width + x.
const TOP_STAR_ALLOW: &[u8; SCREEN_W * SCREEN_H / 8] =
    include_bytes!("../assets/runtime/top_twinkle_allow.bin");
const BOTTOM_STAR_ALLOW: &[u8; BOTTOM_W * BOTTOM_H / 8] =
    include_bytes!("../assets/runtime/bottom_twinkle_allow.bin");
const TWINKLE_STEP_MS: u128 = 90;

#[derive(Clone, Copy)]
struct StarTwinkle {
    x: usize,
    y: usize,
    radius: usize,
    period: u32,
    phase: u32,
    strength: u8,
}
const TOP_STARS: [StarTwinkle; 24] = [
    StarTwinkle {
        x: 14,
        y: 22,
        radius: 7,
        period: 35,
        phase: 21,
        strength: 175,
    },
    StarTwinkle {
        x: 36,
        y: 27,
        radius: 7,
        period: 42,
        phase: 32,
        strength: 192,
    },
    StarTwinkle {
        x: 42,
        y: 57,
        radius: 7,
        period: 49,
        phase: 43,
        strength: 209,
    },
    StarTwinkle {
        x: 68,
        y: 81,
        radius: 7,
        period: 31,
        phase: 23,
        strength: 180,
    },
    StarTwinkle {
        x: 35,
        y: 94,
        radius: 7,
        period: 38,
        phase: 27,
        strength: 197,
    },
    StarTwinkle {
        x: 14,
        y: 136,
        radius: 7,
        period: 45,
        phase: 31,
        strength: 168,
    },
    StarTwinkle {
        x: 10,
        y: 188,
        radius: 6,
        period: 52,
        phase: 35,
        strength: 185,
    },
    StarTwinkle {
        x: 22,
        y: 229,
        radius: 7,
        period: 34,
        phase: 30,
        strength: 202,
    },
    StarTwinkle {
        x: 28,
        y: 201,
        radius: 6,
        period: 41,
        phase: 27,
        strength: 173,
    },
    StarTwinkle {
        x: 24,
        y: 104,
        radius: 6,
        period: 48,
        phase: 24,
        strength: 190,
    },
    StarTwinkle {
        x: 22,
        y: 76,
        radius: 6,
        period: 30,
        phase: 11,
        strength: 207,
    },
    StarTwinkle {
        x: 374,
        y: 225,
        radius: 7,
        period: 37,
        phase: 31,
        strength: 178,
    },
    StarTwinkle {
        x: 378,
        y: 193,
        radius: 7,
        period: 44,
        phase: 21,
        strength: 195,
    },
    StarTwinkle {
        x: 380,
        y: 103,
        radius: 7,
        period: 51,
        phase: 11,
        strength: 212,
    },
    StarTwinkle {
        x: 371,
        y: 67,
        radius: 7,
        period: 33,
        phase: 10,
        strength: 183,
    },
    StarTwinkle {
        x: 342,
        y: 37,
        radius: 7,
        period: 40,
        phase: 26,
        strength: 200,
    },
    StarTwinkle {
        x: 323,
        y: 52,
        radius: 7,
        period: 47,
        phase: 9,
        strength: 171,
    },
    StarTwinkle {
        x: 346,
        y: 123,
        radius: 7,
        period: 29,
        phase: 5,
        strength: 188,
    },
    StarTwinkle {
        x: 370,
        y: 8,
        radius: 6,
        period: 36,
        phase: 3,
        strength: 205,
    },
    StarTwinkle {
        x: 282,
        y: 21,
        radius: 7,
        period: 43,
        phase: 15,
        strength: 176,
    },
    StarTwinkle {
        x: 386,
        y: 38,
        radius: 6,
        period: 50,
        phase: 41,
        strength: 193,
    },
    StarTwinkle {
        x: 382,
        y: 138,
        radius: 6,
        period: 32,
        phase: 28,
        strength: 210,
    },
    StarTwinkle {
        x: 348,
        y: 23,
        radius: 6,
        period: 39,
        phase: 29,
        strength: 181,
    },
    StarTwinkle {
        x: 343,
        y: 98,
        radius: 6,
        period: 46,
        phase: 44,
        strength: 198,
    },
];
const BOTTOM_STARS: [StarTwinkle; 26] = [
    StarTwinkle {
        x: 14,
        y: 17,
        radius: 7,
        period: 31,
        phase: 9,
        strength: 171,
    },
    StarTwinkle {
        x: 32,
        y: 81,
        radius: 7,
        period: 38,
        phase: 20,
        strength: 188,
    },
    StarTwinkle {
        x: 49,
        y: 112,
        radius: 7,
        period: 45,
        phase: 31,
        strength: 205,
    },
    StarTwinkle {
        x: 88,
        y: 126,
        radius: 7,
        period: 52,
        phase: 42,
        strength: 176,
    },
    StarTwinkle {
        x: 127,
        y: 129,
        radius: 7,
        period: 34,
        phase: 19,
        strength: 193,
    },
    StarTwinkle {
        x: 151,
        y: 130,
        radius: 7,
        period: 41,
        phase: 23,
        strength: 210,
    },
    StarTwinkle {
        x: 164,
        y: 141,
        radius: 7,
        period: 48,
        phase: 27,
        strength: 181,
    },
    StarTwinkle {
        x: 204,
        y: 150,
        radius: 7,
        period: 30,
        phase: 26,
        strength: 198,
    },
    StarTwinkle {
        x: 138,
        y: 149,
        radius: 7,
        period: 37,
        phase: 23,
        strength: 169,
    },
    StarTwinkle {
        x: 175,
        y: 134,
        radius: 6,
        period: 44,
        phase: 20,
        strength: 186,
    },
    StarTwinkle {
        x: 124,
        y: 170,
        radius: 7,
        period: 51,
        phase: 17,
        strength: 203,
    },
    StarTwinkle {
        x: 41,
        y: 171,
        radius: 7,
        period: 33,
        phase: 31,
        strength: 174,
    },
    StarTwinkle {
        x: 80,
        y: 193,
        radius: 7,
        period: 40,
        phase: 21,
        strength: 191,
    },
    StarTwinkle {
        x: 69,
        y: 210,
        radius: 7,
        period: 47,
        phase: 11,
        strength: 208,
    },
    StarTwinkle {
        x: 88,
        y: 229,
        radius: 7,
        period: 29,
        phase: 18,
        strength: 179,
    },
    StarTwinkle {
        x: 18,
        y: 229,
        radius: 7,
        period: 36,
        phase: 30,
        strength: 196,
    },
    StarTwinkle {
        x: 51,
        y: 199,
        radius: 7,
        period: 43,
        phase: 13,
        strength: 213,
    },
    StarTwinkle {
        x: 151,
        y: 190,
        radius: 7,
        period: 50,
        phase: 46,
        strength: 184,
    },
    StarTwinkle {
        x: 113,
        y: 220,
        radius: 7,
        period: 32,
        phase: 15,
        strength: 201,
    },
    StarTwinkle {
        x: 299,
        y: 57,
        radius: 7,
        period: 39,
        phase: 23,
        strength: 172,
    },
    StarTwinkle {
        x: 299,
        y: 97,
        radius: 7,
        period: 46,
        phase: 45,
        strength: 189,
    },
    StarTwinkle {
        x: 312,
        y: 27,
        radius: 6,
        period: 28,
        phase: 16,
        strength: 206,
    },
    StarTwinkle {
        x: 236,
        y: 103,
        radius: 7,
        period: 35,
        phase: 6,
        strength: 177,
    },
    StarTwinkle {
        x: 272,
        y: 86,
        radius: 7,
        period: 42,
        phase: 10,
        strength: 194,
    },
    StarTwinkle {
        x: 13,
        y: 125,
        radius: 7,
        period: 49,
        phase: 28,
        strength: 211,
    },
    StarTwinkle {
        x: 274,
        y: 126,
        radius: 6,
        period: 31,
        phase: 5,
        strength: 182,
    },
];

// Touch zones correspond to the three actual gray buttons in the drawing.
// Enlarge these hit areas slightly so a finger/stylus can press them easily.
const TOUCH_CENTERS: [(i32, i32); 3] = [(110, 25), (160, 37), (209, 25)];
const TOUCH_RADIUS_X: i32 = 23;
const TOUCH_RADIUS_Y: i32 = 24;

// Bottom-left legend: only Tamagotchi Y/A/B and START. Speed controls
// live exclusively inside CHEAT MENU; the usual shell stays untouched.
const UI_X: usize = 6;
const UI_Y: usize = 108;
const UI_W: usize = 198;
const UI_H: usize = 124;
const CONTROLS_UI: &[u8; UI_W * UI_H * 4] =
    include_bytes!("../assets/runtime/controls_ui_overlay.rgba");

// CHEAT MENU panel, all in the bottom screen's visible pixel coordinates.
// The menu artwork is prerendered RGBA, like the v7 controls legend.
// The Wonder Garden Gotchi Points field has been verified by an actual
// in-game purchase. Travel/care fields remain display-only, NOT mapped.
const MENU_X: usize = 10;
const MENU_Y: usize = 43;
const MENU_W: usize = 300;
const MENU_H: usize = 193;
const PLANET_MENU: &[u8; MENU_W * MENU_H * 4] =
    include_bytes!("../assets/runtime/planet_menu.rgba");

// English ON Wonder Garden field verified on hardware by a real balance change
// from 300 to 100 after spending 200. Writes affect ONLY the live SRAM value;
// they are not automatically persisted to the toy's flash save.
const GP_PROBE_ADDR: u32 = 0xF800_D884;
const GP_SRAM_BASE: u32 = 0xF800_0000;
const GP_GLYPH_W: usize = 8;
const GP_GLYPH_H: usize = 12;
const GP_GLYPH_CHARS: &[u8] = b"0123456789ABCDEFx?- /N";
const GP_GLYPHS: &[u8; GP_GLYPH_W * GP_GLYPH_H * 4 * 22] =
    include_bytes!("../assets/runtime/gp_glyph_atlas.rgba");

/// A 32-bit little-endian Gotchi Points value in emulated SRAM.
#[derive(Clone, Copy, PartialEq, Eq)]
struct GpProbe(u32);

// ON max is 99,999. Never write an implausible or out-of-range value.
const GP_MAX: u32 = 99_999;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuPage {
    Home,
    Gotchi,
}

#[derive(Clone, Copy)]
enum GpCheat {
    Add(u32),
    Max,
}

fn write_gp_amount(machine: &mut Machine, amount: u32) -> bool {
    // A belt-and-braces guard: never use an uninitialised firmware address,
    // an implausible balance, or overflow the game's intended GP range.
    if amount > GP_MAX || !matches!(read_gp_probe(machine), Some(GpProbe(old)) if old <= GP_MAX) {
        return false;
    }
    let offset = (GP_PROBE_ADDR - GP_SRAM_BASE) as usize;
    let Some(bytes) = machine.sys.sram.get_mut(offset..offset + 4) else {
        return false;
    };
    bytes.copy_from_slice(&amount.to_le_bytes());
    machine.sys.changed = true; // allow idle detection to notice the SRAM edit
    true
}

/// Change only the verified live field. Return old and new for one-step undo.
fn apply_gp_cheat(machine: &mut Machine, cheat: GpCheat) -> Option<(u32, u32)> {
    let old = read_gp_probe(machine)?.0;
    if old > GP_MAX {
        return None;
    }
    let new = match cheat {
        GpCheat::Add(points) => old.saturating_add(points).min(GP_MAX),
        GpCheat::Max => GP_MAX,
    };
    if new == old || !write_gp_amount(machine, new) {
        return None;
    }
    Some((old, new))
}

fn read_gp_probe(machine: &Machine) -> Option<GpProbe> {
    let offset = (GP_PROBE_ADDR - GP_SRAM_BASE) as usize;
    let bytes = machine.sys.sram.get(offset..offset + 4)?;
    Some(GpProbe(u32::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3],
    ])))
}

/// Display a verified balance, retaining the fallback hex diagnostic for
/// invalid memory states or an unexpected firmware layout.
fn gp_probe_text(sample: Option<GpProbe>) -> String {
    match sample {
        Some(GpProbe(value)) if value <= GP_MAX => format!("{}", value),
        Some(GpProbe(value)) => format!("0x{:08X}", value),
        None => "N/A".to_string(),
    }
}

// Second page: a standalone GP panel, the same dimensions and style.
const GP_DETAIL_MENU: &[u8; MENU_W * MENU_H * 4] =
    include_bytes!("../assets/runtime/gp_detail_menu.rgba");
const GP_UNDO_X: usize = MENU_X + 95;
const GP_UNDO_Y: usize = MENU_Y + 168;
const GP_UNDO_W: usize = 110;
const GP_UNDO_H: usize = 19;
const GP_UNDO_ON: &[u8; GP_UNDO_W * GP_UNDO_H * 4] =
    include_bytes!("../assets/runtime/gp_undo_on.rgba");
const GP_UNDO_OFF: &[u8; GP_UNDO_W * GP_UNDO_H * 4] =
    include_bytes!("../assets/runtime/gp_undo_off.rgba");
const GP_BIG_W: usize = 16;
const GP_BIG_H: usize = 24;
const GP_BIG_CHARS: &[u8] = b"0123456789, GPNA/";
const GP_BIG_GLYPHS: &[u8; GP_BIG_W * GP_BIG_H * 4 * 17] =
    include_bytes!("../assets/runtime/gp_big_glyph_atlas.rgba");
const GP_BUTTON_Y: usize = MENU_Y + 134;
const GP_BUTTON_H: usize = 27;
const GP_BUTTON_W: usize = 80;

const MENU_SPEED_W: usize = 62;
const MENU_SPEED_H: usize = 27;
const MENU_SPEED_X: usize = MENU_X + 20;
const MENU_SPEED_Y: usize = MENU_Y + 94;
const MENU_SPEED_GAP: usize = 66;
const MENU_SPEED_ON: [&[u8; MENU_SPEED_W * MENU_SPEED_H * 4]; 4] = [
    include_bytes!("../assets/runtime/planet_speed_0_on.rgba"),
    include_bytes!("../assets/runtime/planet_speed_1_on.rgba"),
    include_bytes!("../assets/runtime/planet_speed_2_on.rgba"),
    include_bytes!("../assets/runtime/planet_speed_3_on.rgba"),
];
const MENU_SPEED_OFF: [&[u8; MENU_SPEED_W * MENU_SPEED_H * 4]; 4] = [
    include_bytes!("../assets/runtime/planet_speed_0_off.rgba"),
    include_bytes!("../assets/runtime/planet_speed_1_off.rgba"),
    include_bytes!("../assets/runtime/planet_speed_2_off.rgba"),
    include_bytes!("../assets/runtime/planet_speed_3_off.rgba"),
];
const MENU_TOGGLE_X: usize = MENU_X + 239;
const MENU_TOGGLE_Y: usize = MENU_Y + 64;
const MENU_TOGGLE_W: usize = 48;
const MENU_TOGGLE_H: usize = 23;
const MENU_TOGGLE_ON: &[u8; MENU_TOGGLE_W * MENU_TOGGLE_H * 4] =
    include_bytes!("../assets/runtime/planet_toggle_on.rgba");
const MENU_TOGGLE_OFF: &[u8; MENU_TOGGLE_W * MENU_TOGGLE_H * 4] =
    include_bytes!("../assets/runtime/planet_toggle_off.rgba");

/// A generous ellipse over the sleeping planet, safely below the shell buttons.
fn touched_planet(x: i32, y: i32) -> bool {
    let dx = x - 282;
    let dy = y - 202;
    dx * dx * 48 * 48 + dy * dy * 46 * 46 <= 46 * 46 * 48 * 48
}

fn touch_rect(x: i32, y: i32, x0: usize, y0: usize, w: usize, h: usize) -> bool {
    x >= x0 as i32 && x < (x0 + w) as i32 && y >= y0 as i32 && y < (y0 + h) as i32
}

// The emulated chip runs at 96 MHz; one 60 Hz frame is 1.6 million
// instructions. run() takes an ABSOLUTE instruction count, so each loop passes
// "executed so far + this budget".
const INSNS_PER_FRAME: u64 = (CPU_HZ / 60.0) as u64;

// A tap is held at least this long (emulated seconds): the firmware debounces
// buttons, so a very short press would be ignored. Same value as the desktop app.
const MIN_HOLD: f64 = 0.05;

// Speeds of the toy's clock, selected with L (slower) and R (faster).
const TIME_STEPS: [f64; 4] = [1.0, 10.0, 60.0, 300.0];

// Audio settings.
const AUDIO_RATE: u32 = audio::SAMPLE_RATE;
const AUDIO_BUFFERS: usize = 4;
const AUDIO_BUF_CAP: usize = audio::CHUNK_SAMPLES;
const VOLUME: f64 = 0.5; // 0.0 = silent, 1.0 = loudest; lower it if it is too loud

// ---------------------------------------------------------------------------
// Saving
// ---------------------------------------------------------------------------
// The toy keeps its name, settings and progress in its (emulated) flash chip.
// We copy that flash, plus the clock info, to the SD card using the core's own
// save module (the same files as the desktop app: FILE and FILE.json).
// Two slots are used in turn, so a crash or an empty battery in the middle of
// a save can never destroy the previous good save. firmware.bin is never written.
// v40: paths come from the chosen firmware, not a shared constant.
// Save when the toy has stopped changing its flash for this long ...
const SAVE_QUIET_SECS: f64 = 3.0;
// ... but never more often than this (a save takes a moment).
const SAVE_MIN_GAP_SECS: f64 = 30.0;

struct LoadedSave {
    flash: Vec<u8>,
    meta: Option<save::SaveMeta>,
    slot: usize,
}

/// Look at both slots and return the newest usable save, if any.
fn load_newest_save(image: &[u8], save_slots: &[String; 2]) -> Option<LoadedSave> {
    let mut order: Vec<(usize, Option<save::SaveMeta>)> = (0..save_slots.len())
        .map(|s| (s, save::read_meta(&save_slots[s])))
        .collect();
    // newest first (a slot without clock info counts as oldest)
    order.sort_by(|a, b| {
        let ta = a.1.as_ref().map(|m| m.saved_at).unwrap_or(0.0);
        let tb = b.1.as_ref().map(|m| m.saved_at).unwrap_or(0.0);
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });
    for (slot, meta) in order {
        // Ok(Some) = a valid save for this firmware; missing, wrong size or
        // from another firmware version are all skipped.
        if let Ok(Some(flash)) = save::load_flash(&save_slots[slot], image) {
            return Some(LoadedSave { flash, meta, slot });
        }
    }
    None
}

/// A save that is being written by a background thread.
struct PendingSave {
    handle: std::thread::JoinHandle<std::io::Result<()>>,
    slot: usize,
    started: Instant,
}

/// Start saving into the slot that is NOT the newest one. The flash is copied
/// first (fast); the slow writing to the SD card happens in a background
/// thread, so the emulation does not freeze. The other slot stays untouched.
fn start_save(
    machine: &Machine,
    newest: Option<usize>,
    save_slots: &[String; 2],
) -> Result<PendingSave, String> {
    let slot = match newest {
        Some(s) => 1 - s,
        None => 0,
    };
    let path = save_slots[slot].clone();
    let asleep = machine.sys.powered_off;
    let meta = save::meta(machine, asleep, asleep);
    let flash = machine.sys.ram[..tg18::FLASH_SIZE].to_vec();
    let handle = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            // The SD card does not overwrite on rename, so clear the target (and
            // any leftovers of an interrupted save) first.
            for suffix in ["", ".json", ".tmp", ".json.tmp"] {
                let _ = fs::remove_file(format!("{}{}", path, suffix));
            }
            save::write(&flash, &meta, &path)
        })
        .map_err(|e| format!("save FAILED (thread): {}", e))?;
    Ok(PendingSave {
        handle,
        slot,
        started: Instant::now(),
    })
}

/// Wait for a background save to end (call it when it has finished, or at exit).
/// Returns a status line and whether it worked.
fn finish_save(p: PendingSave, newest: &mut Option<usize>, t0: Instant) -> (String, bool) {
    let secs = p.started.elapsed().as_secs_f64();
    match p.handle.join() {
        Ok(Ok(())) => {
            *newest = Some(p.slot);
            (
                format!(
                    "saved to slot {} ({:.0} s) at {:.0} s",
                    p.slot + 1,
                    secs,
                    t0.elapsed().as_secs_f64()
                ),
                true,
            )
        }
        Ok(Err(e)) => (format!("save FAILED: {}", e), false),
        Err(_) => ("save FAILED (thread crashed)".to_string(), false),
    }
}

// ---------------------------------------------------------------------------
// Audio buffer type
// ---------------------------------------------------------------------------

/// One audio buffer in LINEAR memory (the DSP can only read from there).
/// Stereo 16-bit: each sample is a [left, right] pair. Same shape as the
/// Buffer in ctru-rs's examples/audio-filters.rs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Buffer(Vec<[i16; 2], LinearAllocator>);

// ---------------------------------------------------------------------------
// Drawing (top screen)
// ---------------------------------------------------------------------------

// Draw from a cached *darkened* background image; restoring the original
// backdrop within each small star patch prevents leftover bright pixels.
// This never touches a shell or LCD pixel because the masks guard every write.
fn twinkle_allowed(mask: &[u8], width: usize, x: usize, y: usize) -> bool {
    let p = y * width + x;
    (mask[p / 8] & (1 << (p % 8))) != 0
}

// A triangular brightness pulse with a soft, squared fade. Per-star phases
// and periods avoid all stars twinkling simultaneously. No allocations.
fn star_intensity(star: StarTwinkle, tick: u128) -> u32 {
    let p = star.period;
    let t = ((tick as u32).wrapping_add(star.phase)) % p;
    let triangle = if t * 2 <= p {
        510 * t / p
    } else {
        510 * (p - t) / p
    };
    let active = triangle.saturating_sub(48);
    // Bright portion of the pulse only, smooth rise/fall, a little stronger than v25.
    ((active * active * star.strength as u32) / (150 * 150)).min(255)
}

fn twinkle_patch(
    frame: &mut [u8],
    base: &[u8],
    width: usize,
    height: usize,
    allow: &[u8],
    star: StarTwinkle,
    tick: u128,
) {
    let strength = star_intensity(star, tick).min(255);
    let rad = star.radius;
    let rr = (rad * rad) as u32;
    let x0 = star.x.saturating_sub(rad);
    let y0 = star.y.saturating_sub(rad);
    let x1 = (star.x + rad).min(width - 1);
    let y1 = (star.y + rad).min(height - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if !twinkle_allowed(allow, width, x, y) {
                continue;
            }
            let dx = x.abs_diff(star.x) as u32;
            let dy = y.abs_diff(star.y) as u32;
            let d2 = dx * dx + dy * dy;
            if d2 > rr {
                continue;
            }
            let falloff = (rr - d2) * 255 / rr;
            let falloff = (falloff * falloff + 127) / 255;
            // Multiply the existing colors, not blend toward white. A dark
            // background pixel stays dark; bright star arms and glows become
            // brighter while retaining their exact original shape and hue.
            let pos = (x * height + (height - 1 - y)) * 3;
            let boost = (strength * falloff / 255).min(255);
            for ch in 0..3 {
                let old = base[pos + ch] as u32;
                frame[pos + ch] = (old + old * boost / 255).min(255) as u8;
            }
        }
    }
}

fn top_twinkles(frame: &mut [u8], base: &[u8], tick: u128) {
    for &star in TOP_STARS.iter() {
        twinkle_patch(frame, base, SCREEN_W, SCREEN_H, TOP_STAR_ALLOW, star, tick);
    }
}

fn bottom_twinkles(frame: &mut [u8], tick: u128) {
    for &star in BOTTOM_STARS.iter() {
        twinkle_patch(
            frame,
            BOTTOM_BG,
            BOTTOM_W,
            BOTTOM_H,
            BOTTOM_STAR_ALLOW,
            star,
            tick,
        );
    }
}

/// Which 3DS buttons stand for each Tamagotchi button.
fn key_mask(key: Key) -> KeyPad {
    match key {
        Key::A => KeyPad::DPAD_LEFT | KeyPad::Y,
        Key::B => KeyPad::A | KeyPad::DPAD_DOWN,
        Key::C => KeyPad::B | KeyPad::DPAD_RIGHT,
    }
}

/// Reset top: starfield plus smoothed shell, excluding the live LCD patch.
fn fill_background(buf: &mut [u8]) {
    buf.copy_from_slice(TOP_BG);
}

/// Return 0, 1 or 2 for stylus taps on the visible shell buttons.
/// The ellipse is a little larger than the real artwork to improve hit testing.
fn touched_button(x: i32, y: i32) -> Option<usize> {
    for (index, &(cx, cy)) in TOUCH_CENTERS.iter().enumerate() {
        let dx = x - cx;
        let dy = y - cy;
        let nx = dx * dx * TOUCH_RADIUS_Y * TOUCH_RADIUS_Y;
        let ny = dy * dy * TOUCH_RADIUS_X * TOUCH_RADIUS_X;
        if nx + ny <= TOUCH_RADIUS_X * TOUCH_RADIUS_X * TOUCH_RADIUS_Y * TOUCH_RADIUS_Y {
            return Some(index);
        }
    }
    None
}

/// A rose-pink light inside the pressed shell button. The highlight has
/// a bright center and feathered edge (no debug circle or sharp border).
fn draw_touch_highlight(buf: &mut [u8], cx: i32, cy: i32) {
    const RADIUS: i32 = 25;
    for dy in -RADIUS..=RADIUS {
        for dx in -RADIUS..=RADIUS {
            let x = cx + dx;
            let y = cy + dy;
            if x < 0 || x >= BOTTOM_W as i32 || y < 0 || y >= BOTTOM_H as i32 {
                continue;
            }
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            if dist >= RADIUS as f32 {
                continue;
            }
            let t = 1.0 - dist / RADIUS as f32;
            let softness = t * (2.0 - t);
            let a = 0.82 * softness * softness;
            let p = (x as usize * BOTTOM_H + (BOTTOM_H - 1 - y as usize)) * 3;
            // BGR framebuffer order: RGB rose = (255, 115, 190).
            let target = [190.0_f32, 115.0, 255.0];
            for ch in 0..3 {
                let original = buf[p + ch] as f32;
                buf[p + ch] = (original + (target[ch] - original) * a).round() as u8;
            }
        }
    }
}

/// Alpha-blend a small RGBA overlay into the bottom framebuffer.
fn blend_rgba_bottom(
    buf: &mut [u8],
    dst_x: usize,
    dst_y: usize,
    width: usize,
    height: usize,
    rgba: &[u8],
) {
    for y in 0..height {
        for x in 0..width {
            let tx = dst_x + x;
            let ty = dst_y + y;
            if tx >= BOTTOM_W || ty >= BOTTOM_H {
                continue;
            }
            let src = (y * width + x) * 4;
            let a = rgba[src + 3] as u16;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            let dst = (tx * BOTTOM_H + (BOTTOM_H - 1 - ty)) * 3;
            // RGBA source to BGR destination.
            let rgb = [rgba[src], rgba[src + 1], rgba[src + 2]];
            for ch in 0..3 {
                let dst_ch = 2 - ch;
                buf[dst + dst_ch] =
                    ((buf[dst + dst_ch] as u16 * inv + rgb[ch] as u16 * a + 127) / 255) as u8;
            }
        }
    }
}

fn draw_controls_ui(buf: &mut [u8]) {
    blend_rgba_bottom(buf, UI_X, UI_Y, UI_W, UI_H, CONTROLS_UI);
}

/// Render a short read-only value inside the existing Gotchi Points row.
/// Glyphs are small antialiased RGBA sprites, not a dynamic font dependency.
fn draw_gp_probe(buf: &mut [u8], sample: Option<GpProbe>) {
    let value = gp_probe_text(sample);
    let width = value.len() * GP_GLYPH_W;
    // Right edge aligns with the other value labels in the original v10 menu.
    let mut x = MENU_X + 280 - width;
    let y = MENU_Y + 148;
    let glyph_len = GP_GLYPH_W * GP_GLYPH_H * 4;
    for byte in value.bytes() {
        if let Some(index) = GP_GLYPH_CHARS.iter().position(|&c| c == byte) {
            let start = index * glyph_len;
            blend_rgba_bottom(
                buf,
                x,
                y,
                GP_GLYPH_W,
                GP_GLYPH_H,
                &GP_GLYPHS[start..start + glyph_len],
            );
        }
        x += GP_GLYPH_W;
    }
}

/// Large centered live GP count, using the same anti-aliased baked-in glyph
/// approach as the v11 diagnostic field (no font service on the 3DS).
fn draw_big_gp(buf: &mut [u8], sample: Option<GpProbe>) {
    let text = match sample {
        Some(GpProbe(value)) if value <= GP_MAX => format!("{} GP", value),
        _ => "N/A".to_string(),
    };
    let mut x = MENU_X + (MENU_W - text.len() * GP_BIG_W) / 2;
    let glyph_len = GP_BIG_W * GP_BIG_H * 4;
    for byte in text.bytes() {
        if let Some(index) = GP_BIG_CHARS.iter().position(|&c| c == byte) {
            let start = index * glyph_len;
            blend_rgba_bottom(
                buf,
                x,
                MENU_Y + 79,
                GP_BIG_W,
                GP_BIG_H,
                &GP_BIG_GLYPHS[start..start + glyph_len],
            );
        }
        x += GP_BIG_W;
    }
}

fn draw_gp_detail(buf: &mut [u8], gp_probe: Option<GpProbe>, undo_available: bool) {
    blend_rgba_bottom(buf, MENU_X, MENU_Y, MENU_W, MENU_H, GP_DETAIL_MENU);
    draw_big_gp(buf, gp_probe);
    blend_rgba_bottom(
        buf,
        GP_UNDO_X,
        GP_UNDO_Y,
        GP_UNDO_W,
        GP_UNDO_H,
        if undo_available {
            GP_UNDO_ON
        } else {
            GP_UNDO_OFF
        },
    );
}

/// Overlay the menu without touching the live Tamagotchi display or speed.
/// All UI drawing happens only when menu/speed/touch state changes.
fn draw_planet_menu(
    buf: &mut [u8],
    speed_idx: usize,
    speed_enabled: bool,
    gp_probe: Option<GpProbe>,
) {
    blend_rgba_bottom(buf, MENU_X, MENU_Y, MENU_W, MENU_H, PLANET_MENU);
    if gp_probe.is_some() {
        draw_gp_probe(buf, gp_probe);
    }
    let toggle = if speed_enabled {
        MENU_TOGGLE_ON
    } else {
        MENU_TOGGLE_OFF
    };
    blend_rgba_bottom(
        buf,
        MENU_TOGGLE_X,
        MENU_TOGGLE_Y,
        MENU_TOGGLE_W,
        MENU_TOGGLE_H,
        toggle,
    );
    for i in 0..TIME_STEPS.len() {
        let badge: &[u8] = if i == speed_idx {
            MENU_SPEED_ON[i]
        } else {
            MENU_SPEED_OFF[i]
        };
        blend_rgba_bottom(
            buf,
            MENU_SPEED_X + i * MENU_SPEED_GAP,
            MENU_SPEED_Y,
            MENU_SPEED_W,
            MENU_SPEED_H,
            badge,
        );
    }
}

/// Build the final bottom-screen frame in a caller-owned buffer. Reusing
/// the buffer avoids a 230 KB allocation/deallocation on each UI change.
/// The base image is restored on every redraw so highlights never accumulate.
fn draw_bottom_into(
    buf: &mut [u8],
    pressed: u8,
    speed_idx: usize,
    menu_open: bool,
    speed_enabled: bool,
    gp_probe: Option<GpProbe>,
    menu_page: MenuPage,
    undo_available: bool,
    twinkle_tick: u128,
) {
    debug_assert_eq!(buf.len(), BOTTOM_W * BOTTOM_H * 3);
    buf.copy_from_slice(BOTTOM_BG);
    bottom_twinkles(buf, twinkle_tick);
    if !menu_open {
        draw_controls_ui(buf);
        for (index, &(cx, cy)) in TOUCH_CENTERS.iter().enumerate() {
            if pressed & (1 << index) != 0 {
                draw_touch_highlight(buf, cx, cy);
            }
        }
    }
    if menu_open {
        match menu_page {
            MenuPage::Home => draw_planet_menu(buf, speed_idx, speed_enabled, gp_probe),
            MenuPage::Gotchi => draw_gp_detail(buf, gp_probe, undo_available),
        }
    }
}

/// A new allocation is still fine for the one-time boot/exit fade sequences.
fn make_bottom_frame(
    pressed: u8,
    speed_idx: usize,
    menu_open: bool,
    speed_enabled: bool,
    gp_probe: Option<GpProbe>,
    menu_page: MenuPage,
    undo_available: bool,
    twinkle_tick: u128,
) -> Vec<u8> {
    let mut frame = vec![0; BOTTOM_W * BOTTOM_H * 3];
    draw_bottom_into(
        &mut frame,
        pressed,
        speed_idx,
        menu_open,
        speed_enabled,
        gp_probe,
        menu_page,
        undo_available,
        twinkle_tick,
    );
    frame
}

/// Raw BGR8 image to the bottom screen. Used by boot/fade and normal gameplay.
fn present_bottom_pixels(gfx: &Gfx, pixels: &[u8]) {
    let mut bottom = gfx.bottom_screen.borrow_mut();
    let fb = bottom.raw_framebuffer();
    debug_assert_eq!(fb.width, BOTTOM_H);
    debug_assert_eq!(fb.height, BOTTOM_W);
    debug_assert_eq!(pixels.len(), BOTTOM_W * BOTTOM_H * 3);
    unsafe {
        fb.ptr.copy_from(pixels.as_ptr(), pixels.len());
    }
    bottom.flush_buffers();
}

/// Flat scene keeps the original native LCD placement and shell pixels.
fn render(buf: &mut [u8], pixels: &[u8]) {
    lcd::render(buf, pixels, SHELL_LCD);
}

fn render_stereo_lcd(buf: &mut [u8], pixels: &[u8], shell: &[u8]) {
    lcd::render_stereo(buf, pixels, shell);
}

/// Blend a pink notification pill over a *presented* top-eye framebuffer.
/// Scene buffers stay pristine, so screenshots can never contain the pill.
fn blend_toast_top(frame: &mut [u8], kind: ToastKind, x0: usize) {
    let image: &[u8] = match kind {
        ToastKind::Saved => TOAST_SAVED,
        ToastKind::Photo => TOAST_PHOTO,
        ToastKind::Failed => TOAST_FAILED,
    };
    for y in 0..TOAST_H {
        for x in 0..TOAST_W {
            let tx = x0 + x;
            let ty = TOAST_Y + y;
            if tx >= SCREEN_W || ty >= SCREEN_H {
                continue;
            }
            let src = (y * TOAST_W + x) * 4;
            let a = image[src + 3] as u16;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            let dst = (tx * SCREEN_H + (SCREEN_H - 1 - ty)) * 3;
            for ch in 0..3 {
                let color = image[src + (2 - ch)] as u16;
                frame[dst + ch] = ((frame[dst + ch] as u16 * inv + color * a + 127) / 255) as u8;
            }
        }
    }
}

/// Ordinary gameplay display. The final pill is the foremost 3D layer,
/// but is NEVER included in the source buffers captured by SELECT.
fn present_stereo_toast(
    gfx_3d: &mut TopScreen3D<'_>,
    left: &[u8],
    right: &[u8],
    toast: Option<ToastKind>,
    stereo: bool,
) {
    let (mut ls, mut rs) = gfx_3d.split_mut();
    let lf = ls.raw_framebuffer();
    let rf = rs.raw_framebuffer();
    unsafe {
        lf.ptr.copy_from(left.as_ptr(), left.len());
        rf.ptr.copy_from(right.as_ptr(), right.len());
        if let Some(kind) = toast {
            let len = SCREEN_W * SCREEN_H * 3;
            let l = std::slice::from_raw_parts_mut(lf.ptr, len);
            let r = std::slice::from_raw_parts_mut(rf.ptr, len);
            // Foremost layer, positioned away from the raised shell to
            // avoid conflicting parallax at the right edge of the pill.
            let shift = if stereo { 3 } else { 0 };
            blend_toast_top(l, kind, TOAST_X + shift);
            blend_toast_top(r, kind, TOAST_X - shift);
        }
    }
    unsafe {
        gfxFlushBuffers();
    }
}

/// Write both top-eye framebuffers with no HUD text. The top screen and 3D
/// depth layers remain exactly as they were in v27.
fn present_stereo(gfx_3d: &mut TopScreen3D<'_>, left: &[u8], right: &[u8]) {
    {
        let (mut ls, mut rs) = gfx_3d.split_mut();
        let lf = ls.raw_framebuffer();
        let rf = rs.raw_framebuffer();
        unsafe {
            lf.ptr.copy_from(left.as_ptr(), left.len());
            rf.ptr.copy_from(right.as_ptr(), right.len());
        }
    }
    unsafe {
        gfxFlushBuffers();
    }
}

/// Multiply every BGR8 component by an opacity (0 = black, 255 = original).
/// Pixel order and dimensions stay unchanged for both eye buffers.
fn scale_to_black(source: &[u8], destination: &mut [u8], opacity: u16) {
    debug_assert_eq!(source.len(), destination.len());
    for (dst, &src) in destination.iter_mut().zip(source.iter()) {
        *dst = ((src as u16 * opacity + 127) / 255) as u8;
    }
}

/// Fade both LCD screens together at the refresh rate; both stereo eyes get
/// the SAME fade amount, so the 3D scene remains stable as it appears/disappears.
fn fade_both_screens(
    gfx: &Gfx,
    top_3d: &mut TopScreen3D<'_>,
    source_left: &[u8],
    source_right: &[u8],
    source_bottom: &[u8],
    fade_in: bool,
    frames: usize,
) {
    let mut tmp_l = vec![0u8; SCREEN_W * SCREEN_H * 3];
    let mut tmp_r = vec![0u8; SCREEN_W * SCREEN_H * 3];
    let mut tmp_b = vec![0u8; BOTTOM_W * BOTTOM_H * 3];
    for i in 0..=frames {
        let visible = if fade_in { i } else { frames - i };
        let opacity = (visible * 255 / frames) as u16;
        scale_to_black(source_left, &mut tmp_l, opacity);
        scale_to_black(source_right, &mut tmp_r, opacity);
        scale_to_black(source_bottom, &mut tmp_b, opacity);
        present_stereo(top_3d, &tmp_l, &tmp_r);
        present_bottom_pixels(gfx, &tmp_b);
        gfx.wait_for_vblank();
    }
}

// The console is used only for fatal startup errors; regular gameplay uses
// the two graphical backgrounds and never writes text to either screen.
fn startup_error(message: &str, apt: &Apt, hid: &mut Hid, gfx: &Gfx) {
    let _console = Console::new(gfx.bottom_screen.borrow_mut());
    println!("{}", message);
    println!("Press START to exit.");
    wait_for_exit(apt, hid, gfx);
}

fn main() {
    // Ask for the New 3DS fast CPU mode.
    unsafe {
        osSetSpeedupEnable(true);
    }

    let apt = Apt::new().unwrap();
    let mut hid = Hid::new().unwrap();
    let gfx = Gfx::new().unwrap();
    // Start both LCDs in black BEFORE loading firmware or starting audio.
    // The old version drew the bottom galaxy immediately while the top remained
    // uninitialized, causing a distracting split-screen startup.
    let bottom_dimensions = {
        let mut bottom = gfx.bottom_screen.borrow_mut();
        bottom.set_double_buffering(false);
        bottom.swap_buffers();
        let fb = bottom.raw_framebuffer();
        (fb.width, fb.height)
    };
    if bottom_dimensions != (BOTTOM_H, BOTTOM_W) {
        startup_error(
            "Unexpected bottom framebuffer geometry.",
            &apt,
            &mut hid,
            &gfx,
        );
        return;
    }
    {
        let mut top = gfx.top_screen.borrow_mut();
        top.set_double_buffering(false);
        top.swap_buffers();
    }
    // The top is in stereoscopic mode throughout, including at boot. Its two
    // eye buffers show exactly the same boot logo, without stereo disparity.
    let mut top_3d = TopScreen3D::from(&gfx.top_screen);
    top_3d.set_double_buffering(false);
    top_3d.swap_buffers();
    {
        let (mut left, mut right) = top_3d.split_mut();
        let lf = left.raw_framebuffer();
        let rf = right.raw_framebuffer();
        if (lf.width, lf.height) != (SCREEN_H, SCREEN_W)
            || (rf.width, rf.height) != (SCREEN_H, SCREEN_W)
        {
            startup_error("Unexpected top framebuffer geometry.", &apt, &mut hid, &gfx);
            return;
        }
    }
    let black_top = vec![0u8; SCREEN_W * SCREEN_H * 3];
    let black_bottom = vec![0u8; BOTTOM_W * BOTTOM_H * 3];
    present_stereo(&mut top_3d, &black_top, &black_top);
    present_bottom_pixels(&gfx, &black_bottom);
    gfx.wait_for_vblank();

    // Show the original logo, inverted to white on black, during preparation.
    // Bottom screen stays pure black until the full emulator scene is ready.
    present_stereo(&mut top_3d, BOOT_LOGO, BOOT_LOGO);
    gfx.wait_for_vblank();
    let logo_start = Instant::now();

    // The logo stays on the TOP screen while the BOTTOM screen lists only
    // firmware images present on the SD card. Game startup begins on selection.
    let available = firmware_picker::discover();
    let selected = match firmware_picker::select(&apt, &mut hid, &gfx, &available) {
        Some(index) => index,
        None => return, // START from the picker exits to the Homebrew Launcher.
    };
    let choice = &available[selected];
    // Only English ON Wonder Garden, Magic and Fairy share the proven GP field.
    // Japanese Meets versions are deliberately read-only for this address.
    let gp_cheat_supported = choice.id <= 2;
    let save_slots = match firmware_picker::save_slots(choice.id) {
        Ok(paths) => paths,
        Err(err) => {
            startup_error(&err, &apt, &mut hid, &gfx);
            return;
        }
    };
    // Show a black bottom screen while the selected firmware is being loaded;
    // do not reveal the other game's LCD during the boot transition.
    present_bottom_pixels(&gfx, &black_bottom);
    let bytes = match fs::read(&choice.path) {
        Ok(bytes) => bytes,
        Err(err) => {
            startup_error(
                &format!("Firmware read failed: {}", err),
                &apt,
                &mut hid,
                &gfx,
            );
            return;
        }
    };

    if !tg18::is_image(&bytes) {
        startup_error("Invalid firmware format.", &apt, &mut hid, &gfx);
        return;
    }

    let image = Arc::new(bytes);

    // Start from the newest save if there is one, otherwise from the original firmware.
    let loaded = load_newest_save(&image, &save_slots);
    let mut newest_slot: Option<usize> = loaded.as_ref().map(|s| s.slot);
    let mut machine = match loaded.as_ref() {
        Some(s) => {
            let mut m = Machine::new(image.clone(), &s.flash, rtc_seconds_now());
            if let Some(meta) = s.meta.as_ref() {
                // Carry the clock over, plus the real time since the save.
                save::restore_rtc(&mut m, meta, true);
            }
            m
        }
        None => Machine::new(image.clone(), &image, rtc_seconds_now()),
    };
    drop(loaded); // free the second copy of the flash (8 MB)
    let (lcd_w, lcd_h) = Machine::screen_size();
    if (lcd_w, lcd_h) != (LCD_W, LCD_H) {
        startup_error("Expected a 128x128 Tamagotchi LCD.", &apt, &mut hid, &gfx);
        return;
    }

    // Screen dimensions and stereo setup were already checked at the start,
    // while both physical screens were black. Keep the boot logo on display
    // throughout emulation/core initialization and audio setup.
    let mut rendered_left = vec![0u8; SCREEN_W * SCREEN_H * 3];
    let mut rendered_right = vec![0u8; SCREEN_W * SCREEN_H * 3];

    // --- audio setup (waves first, then ndsp, then the channel) ---------------
    let mut waves: Vec<Wave<Buffer>> = Vec::new();
    for _ in 0..AUDIO_BUFFERS {
        let mut data = Buffer(Vec::with_capacity_in(AUDIO_BUF_CAP, LinearAllocator));
        data.0.resize(AUDIO_RATE as usize / 60, [0, 0]); // start with silence
        waves.push(Wave::new(data, AudioFormat::PCM16Stereo, false));
    }

    // Start the audio service. If that fails the app simply runs silently.
    let mut ndsp = match Ndsp::new() {
        Ok(n) => Some(n),
        Err(_) => None,
    };

    // Open and configure the channel.
    let mut channel = match ndsp.as_mut() {
        Some(n) => {
            n.set_output_mode(OutputMode::Stereo);
            match n.channel(0) {
                Ok(c) => Some(c),
                Err(_) => None,
            }
        }
        None => None,
    };
    if let Some(ch) = channel.as_mut() {
        ch.set_interpolation(InterpolationType::Linear);
        ch.set_sample_rate(AUDIO_RATE as f32);
        ch.set_format(AudioFormat::PCM16Stereo);
        let mix = AudioMix::default();
        ch.set_mix(&mix);
    }

    // Starting the audio can switch the New 3DS fast CPU mode off, so ask for it again.
    unsafe {
        osSetSpeedupEnable(true);
    }

    let mut audio_output = audio::NativeAudio::new(VOLUME);
    // Reuse the core's event Vec capacity after each batch is accepted.
    let mut sound_events = Vec::new();
    let mut next_wave: usize = 0; // which buffer to refill next (round robin)

    let mut running = true;
    let mut last_drawn_frames = u64::MAX; // forces a first draw
    let program_start = Instant::now();
    // the toy's clock follows real time (and can be sped up with R)
    let mut speed_idx: usize = 0;
    let mut speed_enabled = false; // L/R inactive until enabled in CHEAT MENU.
    let mut menu_open = false;
    let mut menu_page = MenuPage::Home;
    // Diagnostics only draw over the bottom screen. The 3D photo buffers stay clean.
    let mut diagnostics_open = false;
    let mut previous_debug_chord = false;
    // Baseline at start of the active gameplay loop, then sample counter deltas
    // every iteration (even while the diagnostic overlay is not visible).
    let mut previous_probe_stats = machine.probe_stats;
    let mut previous_idle_skipped = machine.idle_skipped;
    let mut diagnostics_idle_page = false;
    let mut last_diagnostics_idle_page = false;
    let mut last_diagnostics_open = false;
    // Undo only when the balance still equals the value created by that cheat.
    let mut gp_undo: Option<(u32, u32)> = None;
    let mut gp_probe = if gp_cheat_supported {
        read_gp_probe(&machine)
    } else {
        None
    };
    let mut gp_probe_last_drawn = gp_probe;
    let mut gp_probe_last_poll = Instant::now();
    // saving
    let mut last_flash_writes = machine.sys.flash.writes;
    let mut last_rtc_regs = machine.sys.rtc.regs;
    let mut save_dirty = false; // changes the toy made that are not saved yet
    let mut last_change = Instant::now();
    let mut last_save = Instant::now();
    let mut pending: Option<PendingSave> = None; // a save being written in the background
                                                 // Only display the toast after an SD save has actually completed.
    let mut toast_started: Option<Instant> = None;
    let mut toast_kind: Option<ToastKind> = None;
    let mut last_toast_drawn: Option<ToastKind> = None;
    let mut pending_photo: Option<std::thread::JoinHandle<Result<String, String>>> = None;
    let mut exit_via_start = false;

    // The touch is processed as a normal press/release of the emulated keys.
    let mut previous_touch: Option<usize> = None;
    let mut last_button_lights: u8 = 0;
    let mut last_speed_idx = speed_idx;
    let mut last_speed_enabled = speed_enabled;
    let mut last_menu_open = menu_open;
    let mut last_menu_page = menu_page;
    let mut last_undo_available = false;

    // Finish preparing the initial scene before showing either gameplay LCD.
    // This preserves all of the prior LCD+bezel positions and stereo parallax.
    let initial_stereo = ctru::os::current_3d_slider_state() > STEREO_SLIDER_THRESHOLD;
    let initial_dark = machine.sys.powered_off || machine.alarm_wake;
    // Reused whenever the emulated LCD is dark, instead of allocating a new
    // black image during each asleep-screen redraw.
    let blank_lcd = vec![0u8; LCD_W * LCD_H * 3];
    let initial_lcd = if initial_dark {
        None
    } else {
        Some(machine.lcd_rgb())
    };
    let initial_pixels = initial_lcd.as_deref().unwrap_or(&blank_lcd);
    if initial_stereo {
        rendered_left.copy_from_slice(TOP_BG_LEFT);
        rendered_right.copy_from_slice(TOP_BG_RIGHT);
        render_stereo_lcd(&mut rendered_left, &initial_pixels, SHELL_LEFT);
        render_stereo_lcd(&mut rendered_right, &initial_pixels, SHELL_RIGHT);
        top_twinkles(&mut rendered_left, TOP_BG_LEFT, 0);
        top_twinkles(&mut rendered_right, TOP_BG_RIGHT, 0);
    } else {
        fill_background(&mut rendered_left);
        render(&mut rendered_left, &initial_pixels);
        top_twinkles(&mut rendered_left, TOP_BG, 0);
        // Flat mode uses rendered_left for BOTH eyes; no duplicate buffer copy.
    }
    let mut last_stereo = initial_stereo;
    let mut last_dark = initial_dark;
    // Give the logo at least a short readable hold, even with a fast save load.
    while apt.main_loop() && logo_start.elapsed().as_millis() < BOOT_MIN_LOGO_MS {
        gfx.wait_for_vblank();
    }
    // White wordmark -> black -> simultaneous reveal of both finished screens.
    fade_both_screens(
        &gfx,
        &mut top_3d,
        BOOT_LOGO,
        BOOT_LOGO,
        &black_bottom,
        false,
        LOGO_FADE_FRAMES,
    );
    let initial_bottom = make_bottom_frame(
        0,
        speed_idx,
        menu_open,
        speed_enabled,
        gp_probe,
        menu_page,
        false,
        0,
    );
    fade_both_screens(
        &gfx,
        &mut top_3d,
        &rendered_left,
        if initial_stereo {
            &rendered_right
        } else {
            &rendered_left
        },
        &initial_bottom,
        true,
        SCENE_FADE_FRAMES,
    );
    present_bottom_pixels(&gfx, &initial_bottom);
    // Boot-only images no longer need to occupy RAM during emulation.
    drop(initial_bottom);
    drop(black_top);
    drop(black_bottom);
    drop(initial_lcd);
    // This screen-sized scratch buffer is shared by every normal UI redraw.
    let mut bottom_frame_scratch = vec![0u8; BOTTOM_W * BOTTOM_H * 3];
    let twinkle_origin = Instant::now();
    let mut last_twinkle_tick: u128 = 0;
    // Exclude the cinematic intro from the toy's real-time clock bookkeeping.
    let mut clock_wall = Instant::now();
    // Prime a short lead AFTER the fades; waves queued at boot have already
    // drained by now. Two silent chunks cushion routine CPU/vblank jitter.
    if let Some(ch) = channel.as_mut() {
        for (index, wave) in waves.iter_mut().take(2).enumerate() {
            let data = wave.get_buffer();
            unsafe {
                if DSP_FlushDataCache(data.as_ptr().cast(), data.len() as u32) != 0 {
                    audio_output.stats.cache_flush_errors += 1;
                }
            }
            if ch.queue_wave(wave).is_ok() {
                next_wave = index + 1;
            } else {
                audio_output.stats.queue_errors += 1;
            }
        }
    }
    let mut profiler = diagnostics::Profiler::new();

    while apt.main_loop() {
        let frame_start = Instant::now();
        let mut emu_us = 0_u64;
        let mut audio_us = 0_u64;
        let mut top_us = 0_u64;
        let mut bottom_us = 0_u64;
        hid.scan_input();
        let down = hid.keys_down();
        let held = hid.keys_held();
        // The New 3DS uses ZL+ZR; original 3DS consoles can hold X+SELECT.
        // Latch the combination so a long hold toggles only once.
        let debug_chord =
            held.contains(KeyPad::ZL | KeyPad::ZR) || held.contains(KeyPad::X | KeyPad::SELECT);
        if debug_chord && !previous_debug_chord {
            diagnostics_open = !diagnostics_open;
            if diagnostics_open {
                menu_open = false;
                menu_page = MenuPage::Home;
                // Keep the previous 10 seconds of gameplay measurements.
                // Opening the overlay must not erase an earlier transition spike.
            }
        }
        previous_debug_chord = debug_chord;
        // Page switch is local to diagnostics. Do not forward Y to Tamagotchi
        // while this overlay is open.
        if diagnostics_open && down.contains(KeyPad::Y) {
            diagnostics_idle_page = !diagnostics_idle_page;
        }
        let diagnostics_refresh = diagnostics_open && profiler.take_refresh();
        // A planet tap opens the menu. While open, touch belongs exclusively
        // to the overlay; physical Y/A/B still control the live Tamagotchi.
        // On TOUCH down (rather than held) menu actions fire exactly once.
        if down.contains(KeyPad::TOUCH) {
            let (tx, ty) = hid.touch_position();
            let x = tx as i32;
            let y = ty as i32;
            if diagnostics_open {
                if diagnostics::close_hit(x, y) {
                    diagnostics_open = false;
                }
            } else if menu_open {
                // Either page has a close button at the top-right.
                if touch_rect(x, y, MENU_X + 265, MENU_Y + 11, 25, 25) {
                    menu_open = false;
                    menu_page = MenuPage::Home;
                } else if menu_page == MenuPage::Gotchi {
                    if touch_rect(x, y, MENU_X + 11, MENU_Y + 11, 25, 25) {
                        menu_page = MenuPage::Home;
                    } else if touch_rect(x, y, GP_UNDO_X, GP_UNDO_Y, GP_UNDO_W, GP_UNDO_H) {
                        // Undo is safe only if the game has not touched GP since
                        // the previous cheat. Never overwrite a newer balance.
                        if let Some((before, after)) = gp_undo.take() {
                            if gp_cheat_supported && read_gp_probe(&machine) == Some(GpProbe(after))
                            {
                                let _ = write_gp_amount(&mut machine, before);
                            }
                        }
                        gp_probe = if gp_cheat_supported {
                            read_gp_probe(&machine)
                        } else {
                            None
                        };
                    } else if touch_rect(x, y, MENU_X + 20, GP_BUTTON_Y, GP_BUTTON_W, GP_BUTTON_H) {
                        gp_undo = if gp_cheat_supported {
                            apply_gp_cheat(&mut machine, GpCheat::Add(1_000))
                        } else {
                            None
                        };
                        gp_probe = if gp_cheat_supported {
                            read_gp_probe(&machine)
                        } else {
                            None
                        };
                    } else if touch_rect(x, y, MENU_X + 110, GP_BUTTON_Y, GP_BUTTON_W, GP_BUTTON_H)
                    {
                        gp_undo = if gp_cheat_supported {
                            apply_gp_cheat(&mut machine, GpCheat::Add(10_000))
                        } else {
                            None
                        };
                        gp_probe = if gp_cheat_supported {
                            read_gp_probe(&machine)
                        } else {
                            None
                        };
                    } else if touch_rect(x, y, MENU_X + 200, GP_BUTTON_Y, GP_BUTTON_W, GP_BUTTON_H)
                    {
                        gp_undo = if gp_cheat_supported {
                            apply_gp_cheat(&mut machine, GpCheat::Max)
                        } else {
                            None
                        };
                        gp_probe = if gp_cheat_supported {
                            read_gp_probe(&machine)
                        } else {
                            None
                        };
                    }
                } else if touch_rect(x, y, MENU_X + 20, MENU_Y + 145, 260, 19) {
                    // Japanese firmware: do not enter an editor for unmapped SRAM.
                    if gp_cheat_supported {
                        menu_page = MenuPage::Gotchi;
                        gp_probe = read_gp_probe(&machine);
                    }
                } else if touch_rect(
                    x,
                    y,
                    MENU_TOGGLE_X,
                    MENU_TOGGLE_Y,
                    MENU_TOGGLE_W,
                    MENU_TOGGLE_H,
                ) {
                    speed_enabled = !speed_enabled;
                    if !speed_enabled {
                        speed_idx = 0; // OFF means real-time x1; no hidden turbo.
                    }
                } else {
                    for i in 0..TIME_STEPS.len() {
                        if touch_rect(
                            x,
                            y,
                            MENU_SPEED_X + i * MENU_SPEED_GAP,
                            MENU_SPEED_Y,
                            MENU_SPEED_W,
                            MENU_SPEED_H,
                        ) {
                            speed_idx = i;
                            speed_enabled = true; // selecting a stage enables it.
                            break;
                        }
                    }
                }
            } else if touched_planet(x, y) {
                menu_open = true;
                menu_page = MenuPage::Home;
            }
        }

        // Touch on the ordinary eggshell buttons only when the menu is shut.
        // (0,0) from a released touch must never count as a press.
        let touch_key: Option<usize> =
            if !menu_open && !diagnostics_open && held.contains(KeyPad::TOUCH) {
                let (x, y) = hid.touch_position();
                touched_button(x as i32, y as i32)
            } else {
                None
            };

        // Y belongs to diagnostics while its page switch is visible. Use the
        // same filtered input for highlights, press edges, and key releases.
        let game_down = if diagnostics_open {
            down & !KeyPad::Y
        } else {
            down
        };
        let game_held = if diagnostics_open {
            held & !KeyPad::Y
        } else {
            held
        };
        let mut button_lights: u8 = 0;
        for key in Key::all() {
            // Never show the eggshell highlights through the open menu.
            if !menu_open && (game_held.intersects(key_mask(key)) || touch_key == Some(key.index()))
            {
                button_lights |= 1 << key.index();
            }
        }

        // Shoulder keys are inert until the feature is enabled *in the menu*.
        // They never enable acceleration on their own, including when the
        // CHEAT MENU is open. Disabling the feature also resets speed to x1.
        if speed_enabled {
            if down.contains(KeyPad::R) {
                speed_idx = (speed_idx + 1).min(TIME_STEPS.len() - 1);
            }
            if down.contains(KeyPad::L) {
                speed_idx = speed_idx.saturating_sub(1);
            }
        }
        let time_mult = if speed_enabled {
            TIME_STEPS[speed_idx]
        } else {
            1.0
        };

        // Sample SRAM when the menu opens and twice a second while visible.
        if menu_open
            && (menu_open != last_menu_open || gp_probe_last_poll.elapsed().as_millis() >= 500)
        {
            gp_probe = if gp_cheat_supported {
                read_gp_probe(&machine)
            } else {
                None
            };
            gp_probe_last_poll = Instant::now();
        }
        // Undo must never overwrite subsequent in-game purchases or earnings.
        if let (Some((_, after)), Some(GpProbe(current))) = (gp_undo, gp_probe) {
            if current != after {
                gp_undo = None;
            }
        }
        let undo_available = gp_undo.is_some();
        let twinkle_tick = twinkle_origin.elapsed().as_millis() / TWINKLE_STEP_MS;
        let twinkle_changed = twinkle_tick != last_twinkle_tick;
        let visible_toast =
            if toast_started.is_some_and(|t| t.elapsed().as_millis() < SAVE_NOTICE_MS) {
                toast_kind
            } else {
                None
            };
        let toast_changed = visible_toast != last_toast_drawn;
        if button_lights != last_button_lights
            || speed_idx != last_speed_idx
            || menu_open != last_menu_open
            || speed_enabled != last_speed_enabled
            || menu_page != last_menu_page
            || undo_available != last_undo_available
            || (menu_open && gp_probe != gp_probe_last_drawn)
            || twinkle_changed
            || diagnostics_open != last_diagnostics_open
            || diagnostics_refresh
            || diagnostics_idle_page != last_diagnostics_idle_page
        {
            let bottom_start = Instant::now();
            draw_bottom_into(
                &mut bottom_frame_scratch,
                button_lights,
                speed_idx,
                menu_open,
                speed_enabled,
                gp_probe,
                menu_page,
                undo_available,
                twinkle_tick,
            );
            if diagnostics_open {
                diagnostics::draw(
                    &mut bottom_frame_scratch,
                    profiler.snapshot(),
                    diagnostics_idle_page,
                );
            }
            present_bottom_pixels(&gfx, &bottom_frame_scratch);
            bottom_us = bottom_start.elapsed().as_micros() as u64;
            gp_probe_last_drawn = gp_probe;
            last_button_lights = button_lights;
            last_speed_idx = speed_idx;
            last_menu_open = menu_open;
            last_speed_enabled = speed_enabled;
            last_menu_page = menu_page;
            last_undo_available = undo_available;
            last_diagnostics_open = diagnostics_open;
            last_diagnostics_idle_page = diagnostics_idle_page;
        }
        if down.contains(KeyPad::START) {
            exit_via_start = true;
            break;
        }
        // SELECT is exclusively for a full-top-screen 3D Camera album photo.
        let take_photo =
            down.contains(KeyPad::SELECT) && !held.contains(KeyPad::X) && pending_photo.is_none();

        // Automatic, subtle 3D effect. The slider controls the mode; at zero,
        // the original pixel-identical top 2D view is restored.
        let stereo = ctru::os::current_3d_slider_state() > STEREO_SLIDER_THRESHOLD;
        let stereo_changed = stereo != last_stereo;
        // Twinkles redraw the finished scene but do not require decoding the
        // emulated LCD again; v13's fast LCD path remains event-driven.
        let mut redraw = stereo_changed || twinkle_changed || toast_changed;
        let mut lcd_redraw = stereo_changed;
        last_stereo = stereo;

        if running {
            // --- buttons -------------------------------------------------
            for key in Key::all() {
                let i = key.index();
                let mask = key_mask(key);
                // Touching the shell is equivalent to a 3DS key press.
                // Moving the stylus to another shell button releases the
                // old key and presses the new one on the same frame.
                let touch_pressed = touch_key == Some(i);
                let touch_new = touch_pressed && previous_touch != Some(i);
                if game_down.intersects(mask) || touch_new {
                    let now = machine.now();
                    machine.sys.live_keys[i] = true;
                    machine.alarm_wake = false; // a button turns the screen on
                    machine.sys.hold_until[i] = now + MIN_HOLD;
                    if machine.sys.powered_off {
                        // a press wakes the toy from sleep
                        machine.sys.key_script.retain(|&(t, _)| t > now - 1.0);
                        machine.press(now, key);
                    }
                } else if !game_held.intersects(mask) && !touch_pressed {
                    machine.sys.live_keys[i] = false;
                }
            }

            previous_touch = touch_key;

            // --- one frame of emulation ------------------------------------
            let emu_begin = Instant::now();
            let t_before = machine.now(); // emulated time at the start of this frame
            let target = machine.executed + INSNS_PER_FRAME;
            machine.sys.turbo = time_mult; // speed of the toy's clock
            if machine.sys.powered_off {
                // asleep: let the clock run on until an alarm or a button wakes it
                if !machine.power_cycle(target) {
                    let turbo = machine.sys.turbo;
                    machine.sys.game_time += (target - machine.executed) as f64 / CPU_HZ * turbo;
                    machine.executed = target;
                    machine.sys.now = target as f64 / CPU_HZ;
                }
            } else {
                match machine.run(target) {
                    // PowerOff just means it fell asleep; the branch above handles it
                    Outcome::Limit | Outcome::PowerOff => {}
                    other => {
                        running = false;
                        let _ = other; // Pause on an emulation fault.
                        redraw = true; // show the final picture
                    }
                }
            }

            emu_us = emu_begin.elapsed().as_micros() as u64;

            // Keep the toy's clock on real time: game_time already moved by the
            // emulated time of this frame; add what real time had more (at most 2 s).
            {
                let real_dt = clock_wall.elapsed().as_secs_f64();
                clock_wall = Instant::now();
                let emu_dt = (machine.now() - t_before).max(0.0) * machine.sys.turbo;
                machine.sys.game_time += (real_dt - emu_dt).clamp(0.0, 2.0);
            }

            // --- native-rate buzzer audio -----------------------------------
            let audio_begin = Instant::now();
            std::mem::swap(&mut sound_events, &mut machine.sys.sound.events);
            audio_output.render(
                &sound_events,
                t_before,
                machine.now(),
                machine.sys.powered_off,
            );
            sound_events.clear();

            if let Some(ch) = channel.as_mut() {
                // Submit complete 735-frame chunks in order. A busy/failed wave
                // leaves its PCM in the FIFO instead of silently dropping notes.
                for _ in 0..AUDIO_BUFFERS {
                    let count = audio_output.chunk_len();
                    if count == 0 {
                        break;
                    }
                    let wave = &mut waves[next_wave];
                    if !matches!(wave.status(), Status::Free | Status::Done) {
                        break;
                    }
                    match wave.get_raw_buffer_mut() {
                        Ok(buffer) => {
                            buffer.0.resize(count, [0, 0]);
                            audio::interleave_mono(audio_output.chunk(), &mut buffer.0);
                        }
                        Err(_) => {
                            audio_output.stats.buffer_errors += 1;
                            break;
                        }
                    }
                    if wave.set_sample_count(count).is_err() {
                        audio_output.stats.sample_count_errors += 1;
                        break;
                    }
                    let data = wave.get_buffer(); // len is BYTES, not stereo frames
                    unsafe {
                        if DSP_FlushDataCache(data.as_ptr().cast(), data.len() as u32) != 0 {
                            audio_output.stats.cache_flush_errors += 1;
                        }
                    }
                    if ch.queue_wave(wave).is_err() {
                        audio_output.stats.queue_errors += 1;
                        break;
                    }
                    audio_output.commit_chunk();
                    next_wave = (next_wave + 1) % waves.len();
                }
            } else {
                audio_output.discard_output();
            }

            audio_us = audio_begin.elapsed().as_micros() as u64;

            // --- saving: did the toy change its saved data? -----------------
            if machine.sys.flash.writes != last_flash_writes
                || machine.sys.rtc.regs != last_rtc_regs
            {
                last_flash_writes = machine.sys.flash.writes;
                last_rtc_regs = machine.sys.rtc.regs;
                save_dirty = true;
                last_change = Instant::now();
            }
            // a background save that has finished?
            if let Some(p) = pending.take() {
                if p.handle.is_finished() {
                    let (_msg, ok) = finish_save(p, &mut newest_slot, program_start);
                    if ok {
                        toast_kind = Some(ToastKind::Saved);
                        toast_started = Some(Instant::now());
                    } else {
                        save_dirty = true; // tried again after the gap
                        toast_kind = Some(ToastKind::Failed);
                        toast_started = Some(Instant::now());
                    }
                } else {
                    pending = Some(p);
                }
            }
            // start a new one when the toy has been quiet for a while
            if pending.is_none()
                && save_dirty
                && last_change.elapsed().as_secs_f64() >= SAVE_QUIET_SECS
                && last_save.elapsed().as_secs_f64() >= SAVE_MIN_GAP_SECS
            {
                match start_save(&machine, newest_slot, &save_slots) {
                    Ok(p) => {
                        save_dirty = false; // the copy being written has all changes so far
                        pending = Some(p);
                    }
                    Err(_msg) => {
                        toast_kind = Some(ToastKind::Failed);
                        toast_started = Some(Instant::now());
                    }
                }
                last_save = Instant::now();
            }
        }

        // Asleep (or woken by the alarm with the backlight off): show black.
        let dark = machine.sys.powered_off || machine.alarm_wake;
        if dark != last_dark {
            last_dark = dark;
            redraw = true;
            lcd_redraw = true;
        }

        // Redraw when the emulated LCD completed a new frame.
        let frames = machine.sys.lcd.frames;
        if frames != last_drawn_frames {
            last_drawn_frames = frames;
            redraw = true;
            lcd_redraw = true;
        }

        if redraw {
            let top_begin = Instant::now();
            // Keep the expensive game LCD conversion tied to actual changes,
            // not to our independently animated galaxy behind the shell.
            if lcd_redraw {
                let live_lcd = if dark { None } else { Some(machine.lcd_rgb()) };
                let pixels = live_lcd.as_deref().unwrap_or(&blank_lcd);
                if stereo {
                    // Restore static stereo backgrounds only when 3D starts.
                    if stereo_changed {
                        rendered_left.copy_from_slice(TOP_BG_LEFT);
                        rendered_right.copy_from_slice(TOP_BG_RIGHT);
                    }
                    render_stereo_lcd(&mut rendered_left, pixels, SHELL_LEFT);
                    render_stereo_lcd(&mut rendered_right, pixels, SHELL_RIGHT);
                } else {
                    if stereo_changed {
                        fill_background(&mut rendered_left);
                    }
                    render(&mut rendered_left, pixels);
                }
            }
            if stereo {
                top_twinkles(&mut rendered_left, TOP_BG_LEFT, twinkle_tick);
                top_twinkles(&mut rendered_right, TOP_BG_RIGHT, twinkle_tick);
            } else {
                top_twinkles(&mut rendered_left, TOP_BG, twinkle_tick);
            }
            // In flat mode the same source image serves both eyes. This avoids
            // an additional 288 KB CPU-side copy on every LCD refresh.
            present_stereo_toast(
                &mut top_3d,
                &rendered_left,
                if stereo {
                    &rendered_right
                } else {
                    &rendered_left
                },
                visible_toast,
                stereo,
            );
            last_toast_drawn = visible_toast;
            top_us = top_begin.elapsed().as_micros() as u64;
        }

        // Capture pristine scene buffers. The foreground toast is only in
        // the physical framebuffer, never in these arrays.
        if take_photo {
            let (left, right) = if stereo {
                (rendered_left.clone(), rendered_right.clone())
            } else {
                // Even with the physical 3D slider off, a saved photograph
                // still has proper stereo imagery for the Camera album.
                let pixels = if dark { None } else { Some(machine.lcd_rgb()) };
                let lcd = pixels.as_deref().unwrap_or(&blank_lcd);
                let mut l = TOP_BG_LEFT.to_vec();
                let mut r = TOP_BG_RIGHT.to_vec();
                render_stereo_lcd(&mut l, lcd, SHELL_LEFT);
                render_stereo_lcd(&mut r, lcd, SHELL_RIGHT);
                top_twinkles(&mut l, TOP_BG_LEFT, twinkle_tick);
                top_twinkles(&mut r, TOP_BG_RIGHT, twinkle_tick);
                (l, r)
            };
            match std::thread::Builder::new()
                .name("camera-mpo".into())
                .spawn(move || photo::save_stereo_photo(left, right))
            {
                Ok(handle) => pending_photo = Some(handle),
                Err(_) => {
                    toast_kind = Some(ToastKind::Failed);
                    toast_started = Some(Instant::now());
                }
            }
        }
        // JPEG/MPO encoding happens off the game loop to protect audio.
        if let Some(handle) = pending_photo.take() {
            if handle.is_finished() {
                let completed = handle.join().ok().and_then(Result::ok).is_some();
                toast_kind = Some(if completed {
                    ToastKind::Photo
                } else {
                    ToastKind::Failed
                });
                toast_started = Some(Instant::now());
            } else {
                pending_photo = Some(handle);
            }
        }

        // The twinkle frame was consumed by the LCD and bottom-screen redraw
        // decisions above. Without updating this, `twinkle_changed` remains
        // true forever after the first star step and causes expensive redraws
        // on EVERY emulation loop (on both Old and New 3DS).
        last_twinkle_tick = twinkle_tick;

        // Keep sampling with the panel CLOSED so we capture room transitions,
        // clock menus and idle spikes without the debug overlay's draw cost.
        // Recording is constant-time; the panel only renders when opened.
        let work_us = frame_start.elapsed().as_micros() as u64;
        let vsync_start = Instant::now();
        gfx.wait_for_vblank();
        let vsync_us = vsync_start.elapsed().as_micros() as u64;
        let frame_us = frame_start.elapsed().as_micros() as u64;
        // Read public idle-skip counters after execution, and record their
        // differences from this loop. No extra CPU tracing is enabled.
        let mut probe_delta = [0u64; 5];
        for i in 0..5 {
            probe_delta[i] = machine.probe_stats[i].saturating_sub(previous_probe_stats[i]);
        }
        let skipped_delta = machine.idle_skipped.saturating_sub(previous_idle_skipped);
        previous_probe_stats = machine.probe_stats;
        previous_idle_skipped = machine.idle_skipped;
        profiler.record(
            [
                emu_us, audio_us, top_us, bottom_us, work_us, vsync_us, frame_us,
            ],
            probe_delta,
            skipped_delta,
        );
    }

    #[cfg(feature = "audio-debug")]
    {
        let _ = fs::write(
            "sdmc:/3ds/tg18-3ds/audio-status.txt",
            format!(
                "native 44100 Hz; chunk 735 stereo frames\n{:?}\npending_samples={}\n",
                audio_output.stats,
                audio_output.pending_samples()
            ),
        );
    }
    // Finish an in-progress Camera write before the homebrew app exits.
    // This avoids truncating MPO/JPG files if START follows SELECT quickly.
    if let Some(handle) = pending_photo.take() {
        let _ = handle.join();
    }
    // On START, finish pending saves and write a final copy before showing a
    // confirmation. An applet/system exit still saves quietly as before.
    if let Some(p) = pending.take() {
        let (_msg, ok) = finish_save(p, &mut newest_slot, program_start);
        if !ok {
            save_dirty = true;
        }
    }
    let mut final_save_ok = false;
    if running && (save_dirty || exit_via_start) {
        match start_save(&machine, newest_slot, &save_slots) {
            Ok(p) => {
                let (_msg, ok) = finish_save(p, &mut newest_slot, program_start);
                final_save_ok = ok;
            }
            Err(_msg) => {} // No success message if writing failed.
        }
    }
    if exit_via_start {
        // Show the normal controls as we fade out, even if the cheat menu
        // was open when START was pressed.
        let bottom_frame = make_bottom_frame(
            last_button_lights,
            speed_idx,
            false,
            speed_enabled,
            gp_probe,
            MenuPage::Home,
            gp_undo.is_some(),
            last_twinkle_tick,
        );
        if final_save_ok {
            // Top-screen foreground pill; underlying images stay pristine.
            present_bottom_pixels(&gfx, &bottom_frame);
            present_stereo_toast(
                &mut top_3d,
                &rendered_left,
                if last_stereo {
                    &rendered_right
                } else {
                    &rendered_left
                },
                Some(ToastKind::Saved),
                last_stereo,
            );
            let shown_since = Instant::now();
            while apt.main_loop() && shown_since.elapsed().as_millis() < SAVE_NOTICE_EXIT_MS {
                gfx.wait_for_vblank();
            }
        }
        // Fade the pristine scene; the save notice does not get baked into art.
        fade_both_screens(
            &gfx,
            &mut top_3d,
            &rendered_left,
            if last_stereo {
                &rendered_right
            } else {
                &rendered_left
            },
            &bottom_frame,
            false,
            EXIT_FADE_FRAMES,
        );
    }
}

fn wait_for_exit(apt: &Apt, hid: &mut Hid, gfx: &Gfx) {
    while apt.main_loop() {
        gfx.wait_for_vblank();
        hid.scan_input();

        if hid.keys_down().contains(KeyPad::START) {
            break;
        }
    }
}

// The DSP reads the buffer as raw bytes. (ctru-rs's example uses the bytemuck
// crate for this; plain pointer casts do the same without a new dependency.)
impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.0.as_ptr() as *const u8, self.0.len() * 4) }
    }
}

impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut u8, self.0.len() * 4) }
    }
}

unsafe impl LinearAllocation for Buffer {}
