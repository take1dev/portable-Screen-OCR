# Screen OCR

Screen OCR is a lightweight, blazing-fast, and 100% portable desktop utility written in Rust. It lives silently in your system tray and allows you to instantly extract text from any part of your screen using a global hotkey, automatically copying the results directly to your clipboard.

## ✨ Features

- **Global Hotkey:** Press `Ctrl+Shift+X` from anywhere to freeze the screen and bring up the selection overlay.
- **Multi-Monitor Support:** The selection overlay seamlessly spans across all connected displays, regardless of differing resolutions or layout configurations.
- **100% Portable:** No installation required! The application silently bundles its OCR engine internally and unpacks securely to your system temp directory on the fly.
- **Smart Image Preprocessing:** Extracts text reliably from dark mode backgrounds, low-contrast UI, or small font sizes by manipulating the images natively in-memory (Catmull-Rom upscaling, Grayscale, Otsu-Thresholding, and brightness inversion).
- **Silent & Unobtrusive:** Operates quietly from the System Tray with zero taskbar presence. Includes native Windows Toast notifications upon successful capture.

## 🚀 Getting Started

### Usage
1. Run `screen-ocr.exe`. A small computer icon will appear in your system tray.
2. Press **`Ctrl + Shift + X`** globally to summon the overlay. Let go to cancel, or click and drag a box over the text you wish to read.
3. If you change your mind and want to cancel the overlay without grabbing text, just press `Escape` or Right-Click anywhere.
4. The text is verified, extracted, and placed onto your clipboard!

### Building from Source

Ensure you have [Rust](https://www.rust-lang.org/) installed, and simply run:

```bash
cargo build --release
```
The compiled, portable binary will be located in `/target/release/screen-ocr.exe`.

## ⚖️ Third-Party Acknowledgements & Licensing

This project is entirely open-source. Please feel free to fork, modify, or contribute.

### Tesseract OCR
This application performs offline local optical character recognition by bundling the extraordinary open-source **Tesseract OCR** engine. 
Tesseract is distributed under the [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0). 
We include pre-compiled binary engines and the `eng` & `srp_latn` trained language data packages.

You may find the Tesseract source and additional model licenses here: [tesseract-ocr/tesseract](https://github.com/tesseract-ocr/tesseract)
