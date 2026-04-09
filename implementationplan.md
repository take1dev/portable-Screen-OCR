# 🖥️ Screen OCR — Rust Implementation Plan

## 1. What the Original App Does

The existing Python application ("Screen OCR") is a **system-tray utility** that lets you:

1. **Press a global hotkey** (`Ctrl+Shift+X`) — from anywhere in Windows.
2. A **full-screen transparent overlay** appears, dimming the screen.
3. You **click-and-drag** to select a region — the selected area is "cut out" (clear) so you can see what you're selecting.
4. On mouse-release the overlay hides, the selected region is **screen-captured** (via `mss`).
5. The captured image is **pre-processed** (upscale 2×, grayscale, Otsu threshold, dark-mode inversion) via OpenCV.
6. **Tesseract OCR** extracts text from the processed image (`eng+srp_latn` languages).
7. The extracted text is **copied to the clipboard**.
8. A **system-tray notification** confirms success.
9. The app lives in the **system tray** with an "Exit" menu item.

### Technology Stack (Python)

| Component | Library |
|---|---|
| GUI / Overlay / Tray | PySide6 (Qt) |
| Global Hotkey | pynput |
| Screen Capture | mss |
| Image Pre-processing | OpenCV + NumPy |
| OCR | pytesseract (wraps native Tesseract) |
| Clipboard | pyperclip |

---

## 2. Rust Equivalent Crate Mapping

| Component | Rust Crate(s) | Notes |
|---|---|---|
| GUI / Overlay Window | **`eframe` / `egui`** | Immediate-mode GUI with full window customization. Use `eframe::NativeOptions` for transparent, frameless, always-on-top. |
| System Tray | **`tray-icon`** + **`muda`** (menu) | Modern, maintained tray crate from the Tauri ecosystem. |
| Global Hotkey | **`global-hotkey`** | Also from the Tauri ecosystem; cross-platform. |
| Screen Capture | **`xcap`** or **`screenshots`** | Pure-Rust multi-monitor screen capture. |
| Image Pre-processing | **`image`** + **`imageproc`** | Grayscale, resize, threshold, inversion — all covered. No OpenCV needed. |
| OCR | **`tesseract`** (C API bindings) or **`leptess`** | Wraps `libtesseract` via FFI. Requires Tesseract + Leptonica C libs. |
| Clipboard | **`arboard`** | Modern, cross-platform clipboard. |
| Notifications | **`notify-rust`** or **`winrt-notification`** | System toast notifications on Windows. |
| Config / Persistence | **`serde` + `toml`** | For user settings (hotkey, language, OCR engine). |

---

## 3. Project Structure

```
printscreen/Rust/
├── Cargo.toml
├── build.rs                  # (optional) link Tesseract/Leptonica
├── assets/
│   ├── icon.png              # Tray icon
│   └── tessdata/             # Bundled Tesseract language data
├── src/
│   ├── main.rs               # Entry point, event loop orchestration
│   ├── hotkey.rs             # Global hotkey registration & listener
│   ├── overlay.rs            # Transparent fullscreen overlay + selection rect
│   ├── capture.rs            # Screen capture of selected region
│   ├── preprocessing.rs      # Image upscale, grayscale, threshold, inversion
│   ├── ocr.rs                # Tesseract OCR wrapper
│   ├── clipboard.rs          # Copy text to system clipboard
│   ├── tray.rs               # System tray icon + menu
│   ├── notification.rs       # Toast / tray notifications
│   ├── config.rs             # User configuration (hotkey, language, etc.)
│   └── history.rs            # (NEW) Capture history with search
└── config.toml               # Default configuration file
```

---

## 4. Implementation Phases

### Phase 1 — Skeleton & System Tray

**Goal:** App launches, shows a system-tray icon, and cleanly exits.

**Files:** `main.rs`, `tray.rs`

**Details:**

- Initialize `winit` event loop (used by `tray-icon` and `global-hotkey`).
- Create tray icon from embedded PNG (`include_bytes!`).
- Add context menu with **"Exit"** item via `muda`.
- Handle tray menu events to cleanly shut down.
- Set up logging with `tracing` or `env_logger` for debugging.

```rust
// main.rs — conceptual skeleton
fn main() {
    let event_loop = winit::event_loop::EventLoop::new();
    let tray = tray::create_tray_icon();
    let hotkey_manager = hotkey::register_hotkeys();

    event_loop.run(move |event, _, control_flow| {
        // Handle tray events, hotkey events, overlay lifecycle
    });
}
```

---

### Phase 2 — Global Hotkey

**Goal:** `Ctrl+Shift+X` is detected globally and triggers an action.

**Files:** `hotkey.rs`, `config.rs`

**Details:**

- Use `global-hotkey` crate to register `Ctrl+Shift+X`.
- On hotkey press → send a message/flag to spawn the overlay.
- Make the hotkey **configurable** via `config.toml`:

```toml
# config.toml
[hotkey]
modifiers = ["Ctrl", "Shift"]
key = "X"

[ocr]
language = "eng"
# language = "eng+srp_latn"  # for multi-language
psm = 6
```

- Parse config at startup with `serde` + `toml`.

---

### Phase 3 — Transparent Overlay & Region Selection

**Goal:** Full-screen transparent overlay with click-drag selection.

**Files:** `overlay.rs`

**Details:**

This is the trickiest part. Two approaches:

#### Approach A — `eframe` / `egui` (Recommended)

- Spawn a new `eframe` window with:
  - `transparent: true`
  - `decorated: false` (frameless)
  - `always_on_top: true`
  - Maximized to cover all monitors
- In `egui::CentralPanel`:
  - Paint a semi-transparent dark rect over the entire area.
  - Track mouse press → drag → release to define selection rect.
  - "Cut out" the selection by painting it with a clear/transparent rect (using `CompositionMode` equivalent in egui's painter).
  - On mouse release → store the `Rect`, close the overlay, trigger capture.

#### Approach B — Raw `winit` + `softbuffer` / `wgpu`

- More control, lower-level. Use only if egui overlay has issues.

**Multi-monitor support:**

- Query all monitors via `winit` or `xcap`.
- Union all monitor geometries to size the overlay.
- Account for per-monitor DPI scaling (critical!).

---

### Phase 4 — Screen Capture

**Goal:** Capture the exact pixel region the user selected.

**Files:** `capture.rs`

**Details:**

- After overlay closes, use `xcap` or `screenshots` to grab the selected region.
- Must apply DPI scaling factor (same logic as the Python app):
  ```rust
  let scale = monitor.scale_factor();
  let physical_rect = Rect {
      x: (logical_rect.x * scale) as i32,
      y: (logical_rect.y * scale) as i32,
      w: (logical_rect.w * scale) as u32,
      h: (logical_rect.h * scale) as u32,
  };
  ```
- Return the captured image as an `image::DynamicImage`.

---

### Phase 5 — Image Pre-processing

**Goal:** Replicate the Python OpenCV pipeline in pure Rust.

**Files:** `preprocessing.rs`

**Details:**

The Python pipeline (and its Rust equivalent):

| Step | Python (OpenCV) | Rust (`image` + `imageproc`) |
|---|---|---|
| **Upscale 2×** | `cv2.resize(..., fx=2, fy=2, INTER_CUBIC)` | `image::imageops::resize(..., Cubic)` |
| **Grayscale** | `cv2.cvtColor(img, COLOR_BGRA2GRAY)` | `img.to_luma8()` |
| **Otsu Threshold** | `cv2.threshold(img, 0, 255, THRESH_BINARY \| THRESH_OTSU)` | Custom Otsu impl or use `imageproc::contrast::otsu_level` + `imageproc::contrast::threshold` |
| **Dark-mode inversion** | `if np.mean(img) < 128: cv2.bitwise_not(img)` | `if mean_brightness < 128 { image::imageops::invert(&mut img) }` |

All of these are straightforward with the `image` and `imageproc` crates — **no OpenCV dependency needed**.

```rust
pub fn preprocess(img: DynamicImage) -> GrayImage {
    // 1. Upscale 2x with cubic interpolation
    let (w, h) = img.dimensions();
    let upscaled = image::imageops::resize(&img, w * 2, h * 2, FilterType::CatmullRom);

    // 2. Convert to grayscale
    let gray = DynamicImage::ImageRgba8(upscaled).to_luma8();

    // 3. Otsu threshold
    let level = imageproc::contrast::otsu_level(&gray);
    let binary = imageproc::contrast::threshold(&gray, level);

    // 4. Invert if dark background
    let mean: f64 = binary.pixels().map(|p| p.0[0] as f64).sum::<f64>()
                     / (binary.width() * binary.height()) as f64;
    if mean < 128.0 {
        image::imageops::invert(&mut binary);
    }

    binary
}
```

---

### Phase 6 — OCR (Tesseract)

**Goal:** Extract text from the pre-processed image.

**Files:** `ocr.rs`

**Details:**

- Use the `tesseract` crate (FFI to libtesseract C API).
- Initialize with language from config (`eng`, `eng+srp_latn`, etc.).
- Set PSM mode from config (default: `6` — single uniform block).
- Feed the pre-processed `GrayImage` as raw bytes.
- **Bundle `tessdata`** in the `assets/` folder; resolve path at runtime.
  - For release builds: embed as a resource or ship alongside the exe.

```rust
pub fn recognize(img: &GrayImage, lang: &str, psm: i32) -> Result<String> {
    let mut tess = tesseract::Tesseract::new(Some("assets/tessdata"), Some(lang))?;
    tess.set_variable("tessedit_pageseg_mode", &psm.to_string())?;
    tess.set_image_from_mem(
        img.as_raw(),
        img.width() as i32,
        img.height() as i32,
        1,
        img.width() as i32,
    )?;
    let text = tess.get_text()?;
    Ok(text)
}
```

**Build requirements:**

- `libtesseract` and `libleptonica` must be available at build time.
- On Windows: use vcpkg (`vcpkg install tesseract:x64-windows`) or ship pre-built DLLs.
- Document this clearly in `README.md`.

---

### Phase 7 — Clipboard & Notification

**Goal:** Copy OCR text to clipboard and notify the user.

**Files:** `clipboard.rs`, `notification.rs`

**Details:**

```rust
// clipboard.rs
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text)?;
    Ok(())
}

// notification.rs (Windows)
pub fn notify_success() {
    winrt_notification::Toast::new("Screen OCR")
        .title("Text Captured")
        .text1("Text copied to clipboard.")
        .duration(winrt_notification::Duration::Short)
        .show()
        .ok();
}
```

---

### Phase 8 — Integration & Polish

**Goal:** Wire everything together in the main event loop.

**Files:** `main.rs`

**Flow:**

```
┌─────────────┐     ┌───────────────┐     ┌──────────┐
│ Global      │────>│ Overlay       │────>│ Capture  │
│ Hotkey      │     │ (select area) │     │ Region   │
└─────────────┘     └───────────────┘     └──────────┘
                                               │
                                               ▼
                                         ┌──────────┐
                                         │ Pre-     │
                                         │ process  │
                                         └──────────┘
                                               │
                                               ▼
                                         ┌──────────┐
                                         │ OCR      │
                                         │ (Tess.)  │
                                         └──────────┘
                                               │
                                               ▼
                                    ┌────────────────────┐
                                    │ Clipboard + Notify │
                                    └────────────────────┘
```

- The OCR + preprocessing should run on a **separate thread** (`std::thread::spawn` or `tokio::spawn_blocking`) to keep the UI responsive.
- Use `std::sync::mpsc` or `crossbeam` channels for inter-thread communication.

---

## 5. New Features (Beyond Python Original)

### 5.1 🔧 Configurable Hotkey

The Python app hardcodes `Ctrl+Shift+X`. The Rust version reads from `config.toml`, allowing the user to change the hotkey without recompiling.

### 5.2 📜 Capture History with Search

**File:** `history.rs`

- Store the last N captures (text + timestamp + thumbnail) in memory.
- Add a tray menu item → **"History"** that opens a small egui window.
- List previous captures with:
  - Timestamp
  - Preview of extracted text (first 100 chars)
  - Click to re-copy to clipboard
  - Search/filter bar
- Optionally persist to a local SQLite database (`rusqlite`) or a JSON file.

### 5.3 🌐 Multi-Language OCR Selection

- The tray menu includes a **"Language"** submenu where the user can pick from installed `tessdata` languages (auto-detected from the `tessdata/` folder).
- Selection saved to `config.toml`.

### 5.4 🖥️ Multi-Monitor DPI Awareness

- The Python app handles DPI with `devicePixelRatio()` but in a basic way.
- The Rust version should enumerate all monitors, detect per-monitor DPI, and correctly map logical overlay coordinates to physical capture coordinates — even on mixed-DPI setups.

### 5.5 ⌨️ Quick Re-capture

- Add a second hotkey (e.g., `Ctrl+Shift+Z`) that **re-captures the last selected region** without showing the overlay. Useful for monitoring a changing value on screen.

### 5.6 📋 Smart Clipboard Formatting

- Auto-trim whitespace and trailing newlines from OCR output.
- Option to strip line breaks (merge into a single paragraph) — useful when capturing multi-line text that should be a single sentence.

---

## 6. Dependencies (`Cargo.toml`)

```toml
[package]
name = "screen-ocr"
version = "0.1.0"
edition = "2021"

[dependencies]
# GUI
eframe = { version = "0.31", features = ["persistence"] }
egui = "0.31"

# System Tray & Menu
tray-icon = "0.19"
muda = "0.16"

# Global Hotkey
global-hotkey = "0.6"

# Screen Capture
xcap = "0.0.14"

# Image Processing
image = "0.25"
imageproc = "0.25"

# OCR
tesseract = "0.15"

# Clipboard
arboard = "3"

# Notifications (Windows)
winrt-notification = "0.6"

# Config
serde = { version = "1", features = ["derive"] }
toml = "0.8"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Error handling
anyhow = "1"
thiserror = "2"
```

> **Note:** Version numbers are approximate — pin to latest at project init time.

---

## 7. Build & Distribution Notes

### Tesseract Dependency

The Tesseract C library and its `tessdata` files must be available:

- **Development (Windows):** Install via `vcpkg`:
  ```
  vcpkg install tesseract:x64-windows-static
  ```
  Set `TESSERACT_INCLUDE_PATHS` and `TESSERACT_LINK_PATHS` env vars.

- **Release:** Ship `tesseract.dll`, `leptonica.dll`, and the `tessdata/` folder alongside the `.exe`, or statically link.

### Compilation

```bash
cargo build --release
```

The resulting binary is a **single `.exe`** (plus the tessdata folder and DLLs). This is a massive improvement over the Python version which requires bundling an entire Python interpreter + Tesseract via PyInstaller (~280MB .rar archives in the original project).

### Installer

Consider using `cargo-wix` to generate a proper Windows `.msi` installer that:
- Copies the exe + tessdata to Program Files
- Creates a Start Menu shortcut
- Optionally adds to startup

---

## 8. Risk Assessment

| Risk | Mitigation |
|---|---|
| Tesseract FFI complexity on Windows | Use `vcpkg` for automated builds; document thoroughly; consider `leptess` as alternative |
| Transparent overlay rendering | Test on multiple GPU drivers; fallback to `winit` + raw rendering if egui has issues |
| Multi-monitor DPI edge cases | Test on mixed-DPI setups; query `winit` scale factors per-monitor |
| OCR accuracy parity with Python | Use identical pre-processing pipeline; compare results side-by-side |
| Binary size | Use `strip`, `lto`, `opt-level = "s"` in release profile; still far smaller than Python bundle |

---

## 9. Success Criteria

- [ ] System tray icon with Exit menu
- [ ] Global hotkey triggers overlay
- [ ] Overlay renders transparent with selection rect
- [ ] Captured region matches the selection exactly (DPI-correct)
- [ ] Pre-processing pipeline produces identical output to Python version
- [ ] Tesseract OCR extracts text accurately
- [ ] Text is copied to clipboard
- [ ] Notification confirms success
- [ ] Config file allows hotkey & language changes
- [ ] Capture history is browsable and searchable
- [ ] Binary size < 20MB (excluding tessdata)
- [ ] Cold-start time < 500ms
