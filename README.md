# Meets On 3DS

**A Tamagotchi Meets and ON emulator for Nintendo 3DS homebrew.**

![Status: Experimental beta](https://img.shields.io/badge/status-experimental%20beta-d97706)
![Platform: Nintendo 3DS](https://img.shields.io/badge/platform-Nintendo%203DS-2563eb)

Bring your virtual pet to the 3DS with physical and touchscreen controls, stereoscopic graphics, and stereo screenshots. This beta includes improved performance and corrected audio.

**AI disclosure:** This frontend was developed with substantial assistance from ChatGPT for coding, debugging, and optimization. I build and test it on my New Nintendo 3DS, but it remains experimental and bugs may still be present. The emulator uses the existing tg18-core project, credited below.

**[Download the experimental beta](https://github.com/maaartennn/Meets-On-3DS/releases/tag/v0.1.0-beta)** · [All releases](https://github.com/maaartennn/Meets-On-3DS/releases) · [Report a bug](https://github.com/maaartennn/Meets-On-3DS/issues)

> **Work in progress:** bugs and incomplete features remain. New Nintendo 3DS is the main tested system. Please back up your saves.
>
> **Firmware is not included.** You must supply your own compatible Tamagotchi firmware dump.

## Source code and building

The source is available directly in this repository:

- [3DS frontend](tg18-3ds/src/) and [included emulator core](tg18-emu-main/core/src/).
- [Embedded assets and artwork](tg18-3ds/assets/).
- [Tests](tg18-3ds/tests/) and [build tools](tg18-3ds/tools/).
- [Original source ZIP](Meets-On-3DS-source-ready.zip).

Keep `tg18-3ds` and `tg18-emu-main` beside each other: Cargo uses the included
core through a relative path. Firmware dumps and personal saves are not included.

Use Rust nightly with the 3DS toolchain, devkitPro's 3DS development tools
(including `3dsxtool` and `smdhtool`), [cargo-3ds](https://github.com/rust3ds/cargo-3ds),
and Python 3.11 or newer. Follow the [rust3ds setup instructions](https://github.com/rust3ds/ctru-rs)
to configure the platform toolchain.

From a clone or extracted repository download:

```bash
cd tg18-3ds
bash build.sh
```

With devkitPro installed at `/opt/devkitpro`:

```bash
PATH="/opt/devkitpro/tools/bin:$PATH" bash build.sh
```

Outputs are `tg18-3ds/READY_FOR_FTPD/tg18-3ds.3dsx` and
`tg18-3ds/READY_FOR_FTPD/tg18-3ds.smdh`. See the
[frontend README](tg18-3ds/README.md) for build options and host test commands.

This is the supplied source snapshot. The existing release binary has not been
independently verified as a byte-for-byte build of this snapshot.

## Features

- Firmware picker for nine English ON and Japanese Meets versions.
- Physical-button and touchscreen controls.
- Autosaving with separate saves for each firmware version.
- Stereoscopic graphics on consoles with a 3D display.
- Stereo screenshots for Nintendo 3DS Camera.
- Clock-speed controls, selected cheats, and performance diagnostics.

## Getting started

You need a Nintendo 3DS or 2DS system capable of launching homebrew, an SD card, and a compatible Tamagotchi firmware dump.

1. Download **Meets-On-3DS-Experimental.zip** from the [release page](https://github.com/maaartennn/Meets-On-3DS/releases/tag/v0.1.0-beta) and extract it.
2. Copy `tg18-3ds.3dsx` to `/3ds/tg18-3ds/` on your SD card.
3. Create `/3ds/tg18-3ds/firmware/` and place your firmware dump there using the matching filename below.
4. Launch **Meets On 3DS** from the Homebrew Launcher.
5. Select your firmware with **D-pad Up/Down + A**, or tap its row.

| Item | SD-card location |
| --- | --- |
| Application | `/3ds/tg18-3ds/tg18-3ds.3dsx` |
| Firmware folder | `/3ds/tg18-3ds/firmware/` |

Artwork and runtime assets are embedded in the application; no separate asset folder is required.

## Supported firmware

The firmware picker supports the following nine versions. Support does not mean every version has been fully tested on every console.

Use a **raw, uncompressed 8 MiB dump** (8,388,608 bytes) with the exact filename for its version. You only need the firmware you intend to use.

<details>
<summary><strong>View supported versions and exact filenames</strong></summary>

| Firmware | Version | Required filename |
| --- | --- | --- |
| English ON Wonder Garden | v063 | `fw_tg18_en_v063_2020-02-13_wondergarden.bin` |
| English ON Magic | v058 | `fw_tg18_en_v058_2019-05-13_magic.bin` |
| English ON Fairy | v057 | `fw_tg18_en_v057_2019-05-13_fairy.bin` |
| Japanese Meets Magic | v031 | `fw_tg18_jp_v031_2018-10-19_magic.bin` |
| Japanese Meets Fairy | v030 | `fw_tg18_jp_v030_2018-10-19_fairy.bin` |
| Japanese Meets Pastel | v055 | `fw_tg18_jp_v055_2019-03-10_pastel.bin` |
| Japanese Meets Fantasy | v047 | `fw_tg18_jp_v047_2019-05-28_fantasy.bin` |
| Japanese Meets Sweets | v062 | `fw_tg18_jp_v062_2019-09-12_sweets.bin` |
| Japanese Meets Sanrio | v056 | `fw_tg18_jp_v056_2019-04-24_sanrio.bin` |

For example, Wonder Garden goes at:

```text
/3ds/tg18-3ds/firmware/fw_tg18_en_v063_2020-02-13_wondergarden.bin
```

Existing Wonder Garden installations can also use the legacy path `/3ds/tg18-3ds/firmware.bin`.

If your firmware does not appear, check the folder, filename, and file size. Renaming a different firmware version does not make it compatible. Listed images must still pass firmware-format validation when launched.

</details>

## Controls

Tamagotchi button names refer to the toy's three buttons, from left to right.

| Action | 3DS control |
| --- | --- |
| Tamagotchi A — left button | **Y**, **D-pad Left**, or left touchscreen button |
| Tamagotchi B — middle button | **A**, **D-pad Down**, or middle touchscreen button |
| Tamagotchi C — right button | **B**, **D-pad Right**, or right touchscreen button |
| Save and exit | **START** |
| Take a stereo screenshot | **SELECT** |
| Adjust stereoscopic presentation | **3D slider**, on compatible consoles |
| Open cheats and clock-speed menu | Tap the planet on the bottom screen |
| Open/close diagnostics | **ZL + ZR** on New 3DS, or **X + SELECT** |
| Switch diagnostics pages | **Y**, while diagnostics is open |

Allow the save/exit sequence to finish before closing the application.

The clock menu offers **1×, 10×, 60×, and 300×**. After enabling clock-speed controls, **L** lowers the speed and **R** raises it. Disabling the feature returns the clock to 1×.

Gotchi Points edits are available for the three English ON versions.

## Saves and screenshots

The emulator autosaves after changes have settled and writes a final save when you exit with **START**.

| Files | SD-card location |
| --- | --- |
| Wonder Garden save slots | `/3ds/tg18-3ds/save1.bin` and `save2.bin`, with matching `.json` metadata |
| Other firmware saves | `/3ds/tg18-3ds/saves/<firmware-id>/` |
| Camera MPO/JPG screenshots | `/DCIM/100NIN03/` |
| Individual left/right JPG screenshots | `/3ds/tg18-3ds/photos/` |

Back up both save slots and their metadata. Keep existing save files in place when updating the application.

Screenshots are generated in stereo even when the 3D slider is off.

## Known limitations

- Hardware testing is primarily on New Nintendo 3DS. Other models may perform differently.
- Not every supported firmware version has been fully tested.
- Some cheat-menu fields, including travel unlocks and care mistakes, are placeholders.
- Persistence of Gotchi Points edits into the toy's own flash save has not been fully verified.
- Bugs may affect timing, performance, or screenshot compatibility.

## Reporting a bug

Please [open an issue](https://github.com/maaartennn/Meets-On-3DS/issues) and include:

- Your console model and the emulator release you are using.
- The Tamagotchi firmware name and version.
- Steps to reproduce the problem.
- What you expected and what happened.
- A screenshot, video, or diagnostics information if useful.

Do not attach firmware dumps or personal save files to public reports.

## Credits

Frontend by [maaartennn](https://github.com/maaartennn).

Built using **tg18-core**, [ctru-rs](https://github.com/rust3ds/ctru-rs), [libctru](https://github.com/devkitPro/libctru), and [jpeg-encoder](https://crates.io/crates/jpeg-encoder).
