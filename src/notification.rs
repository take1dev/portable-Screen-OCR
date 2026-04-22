#[cfg(target_os = "windows")]
pub fn notify_success() {
    winrt_notification::Toast::new("Screen OCR")
        .title("Text Captured")
        .text1("Text copied to clipboard.")
        .duration(winrt_notification::Duration::Short)
        .show()
        .ok();
}

#[cfg(target_os = "windows")]
pub fn notify_error(msg: &str) {
    winrt_notification::Toast::new("Screen OCR")
        .title("Error")
        .text1(msg)
        .duration(winrt_notification::Duration::Short)
        .show()
        .ok();
}

#[cfg(not(target_os = "windows"))]
pub fn notify_success() {
    println!("Text copied to clipboard.");
}

#[cfg(not(target_os = "windows"))]
pub fn notify_error(msg: &str) {
    println!("Error: {}", msg);
}
