#[cfg(target_os = "windows")]
pub fn notify_success() {
    winrt_notification::Toast::new("Screen OCR")
        .title("Text Captured")
        .text1("Text copied to clipboard.")
        .duration(winrt_notification::Duration::Short)
        .show()
        .ok();
}

#[cfg(not(target_os = "windows"))]
pub fn notify_success() {
    // Fallback for other platforms or ignore
    println!("Text copied to clipboard.");
}
