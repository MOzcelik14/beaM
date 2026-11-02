#![no_main]

use libfuzzer_sys::fuzz_target;
use rawler::imgop::develop::{ProcessingStep, RawDevelop};
use rawler::rawsource::RawSource;
use rawler::decoders::RawDecodeParams;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 || data.len() > 10 * 1024 * 1024 {
        return;
    }

    if beam_media::decoder::validate_tiff_raw_header(data).is_err() {
        return;
    }

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let source = RawSource::new_from_slice(data);
        if let Ok(rawimage) = rawler::decode(&source, &RawDecodeParams::default()) {
            let orig_w = rawimage.width as u32;
            let orig_h = rawimage.height as u32;
            if orig_w == 0 || orig_h == 0 {
                return;
            }
            let total_pixels = (orig_w as u64) * (orig_h as u64);
            if total_pixels > beam_media::decoder::MAX_IMAGE_PIXELS {
                return;
            }

            let develop = RawDevelop {
                steps: vec![
                    ProcessingStep::Rescale,
                    ProcessingStep::Demosaic,
                    ProcessingStep::CropActiveArea,
                ],
            };
            let _ = develop.develop_intermediate(&rawimage);
        }
    }));
});
