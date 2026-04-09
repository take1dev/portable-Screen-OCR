use anyhow::Result;
use image::{DynamicImage, RgbaImage};
use xcap::Monitor;

pub fn capture_region(logical_rect: eframe::epaint::Rect, scale_factor: f32, window_offset_x: i32, window_offset_y: i32) -> Result<DynamicImage> {
    // Determine the physical region to capture based on the scale_factor and window offset
    let physical_x = window_offset_x + (logical_rect.min.x * scale_factor) as i32;
    let physical_y = window_offset_y + (logical_rect.min.y * scale_factor) as i32;
    let physical_w = (logical_rect.width() * scale_factor) as u32;
    let physical_h = (logical_rect.height() * scale_factor) as u32;

    let monitors = Monitor::all()?;
    
    // Simplification: In a truly multi-monitor aware capture, we might need to 
    // union all monitors, capture the specific screen where the rect belongs, or 
    // capture all of them and stitch. The original Python app uses mss which captures 
    // a global coordinate space. xcap's monitors each have an x/y offset.
    // For now, let's find the primary monitor or stitch them.
    // Xcap can capture individual monitors. Let's find which monitor contains the top-left point.
    
    for monitor in monitors {
        let mx = monitor.x();
        let my = monitor.y();
        let mw = monitor.width();
        let mh = monitor.height();

        if physical_x >= mx && physical_x < mx + (mw as i32) &&
           physical_y >= my && physical_y < my + (mh as i32) {
            
            // This is the monitor. Capture it.
            let xcap_image = monitor.capture_image()?;
            
            // Convert xcap RgbaImage to image RgbaImage
            let mut img = RgbaImage::from_raw(
                xcap_image.width(),
                xcap_image.height(),
                xcap_image.into_raw(),
            ).expect("Failed to convert image formats");

            // Crop the portion the user selected. 
            // The coordinate inside this monitor image is:
            let local_x = (physical_x - mx) as u32;
            let local_y = (physical_y - my) as u32;
            
            let cropped = image::imageops::crop(&mut img, local_x, local_y, physical_w, physical_h).to_image();
            
            return Ok(DynamicImage::ImageRgba8(cropped));
        }
    }
    
    Err(anyhow::anyhow!("Coordinates not found in any monitor"))
}
