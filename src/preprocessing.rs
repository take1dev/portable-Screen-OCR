use image::{DynamicImage, GrayImage, imageops::FilterType, GenericImageView};
use imageproc::contrast::ThresholdType;

pub fn preprocess(img: DynamicImage) -> GrayImage {
    // 1. Upscale 2x with CatmullRom interpolation for better accuracy
    let (w, h) = img.dimensions();
    let upscaled = image::imageops::resize(&img, w * 2, h * 2, FilterType::CatmullRom);

    // 2. Convert to grayscale
    let gray = DynamicImage::ImageRgba8(upscaled).to_luma8();

    // 3. Otsu threshold
    let level = imageproc::contrast::otsu_level(&gray);
    let mut binary = imageproc::contrast::threshold(&gray, level, ThresholdType::Binary);

    // 4. Invert if dark background
    let mean: f64 = binary.pixels().map(|p| p.0[0] as f64).sum::<f64>()
                     / (binary.width() * binary.height()) as f64;
    if mean < 128.0 {
        image::imageops::invert(&mut binary);
    }

    binary
}
