use anyhow::{Context, Result};
use image::GrayImage;
use std::env;
use std::fs;
use std::io::{Cursor, copy};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

static INIT_TESSERACT: Once = Once::new();

// This embeds the ZIP file right into the .exe
const TESSERACT_ZIP_BYTES: &[u8] = include_bytes!("../assets/tesseract.zip");

pub fn ensure_tesseract_extracted() -> Result<PathBuf> {
    let tesseract_dir = env::temp_dir().join("screen_ocr_tesseract");
    let tesseract_exe = tesseract_dir.join("tesseract.exe");

    let tesseract_dll = tesseract_dir.join("libcurl-4.dll");

    INIT_TESSERACT.call_once(|| {
        if !tesseract_exe.exists() || !tesseract_dll.exists() {
            println!("Extracting bundled Tesseract to {:?}...", tesseract_dir);
            let _ = fs::remove_dir_all(&tesseract_dir); // Clean up partial state
            if let Err(e) = extract_tesseract(&tesseract_dir) {
                eprintln!("Failed to extract Tesseract: {}", e);
            }
        }
    });

    if tesseract_exe.exists() {
        Ok(tesseract_exe)
    } else {
        Err(anyhow::anyhow!("tesseract.exe could not be found or extracted."))
    }
}

fn extract_tesseract(dest_dir: &Path) -> Result<()> {
    if !dest_dir.exists() {
        fs::create_dir_all(dest_dir)?;
    }

    let cursor = Cursor::new(TESSERACT_ZIP_BYTES);
    let mut archive = zip::ZipArchive::new(cursor)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

pub fn recognize(img: &GrayImage, lang: &str, psm: i32) -> Result<String> {
    // Make sure we have our bundled Tesseract extracted and ready!
    let tesseract_exe = ensure_tesseract_extracted()?;
    let tesseract_dir = tesseract_exe.parent().unwrap();

    let temp_dir = env::temp_dir();
    let id = std::process::id();
    
    let img_path = temp_dir.join(format!("screen_ocr_{}_img.png", id));
    let txt_path_base = temp_dir.join(format!("screen_ocr_{}_out", id));
    let txt_path = temp_dir.join(format!("screen_ocr_{}_out.txt", id));

    img.save(&img_path).context("Failed to save temporary image for OCR")?;

    // We must pass --tessdata-dir explicitly to use the bundled models!
    let tessdata_dir = tesseract_dir.join("tessdata");

    let status = Command::new(&tesseract_exe)
        .arg(&img_path)
        .arg(&txt_path_base)
        .arg("--tessdata-dir")
        .arg(&tessdata_dir)
        .arg("-l")
        .arg(lang)
        .arg("--psm")
        .arg(psm.to_string())
        .status()
        .context("Failed to execute bundled tesseract command.")?;

    if !status.success() {
        return Err(anyhow::anyhow!("Tesseract OCR failed with status: {:?}", status));
    }

    let text = fs::read_to_string(&txt_path).context("Failed to read OCR output text")?;

    let _ = fs::remove_file(&img_path);
    let _ = fs::remove_file(&txt_path);

    Ok(text.trim().to_owned())
}
