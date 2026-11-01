#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data));
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    limits.max_alloc = Some(1024 * 1024 * 1024);
    reader.limits(limits);
    if let Ok(reader) = reader.with_guessed_format() {
        let _ = reader.decode();
    }
});
