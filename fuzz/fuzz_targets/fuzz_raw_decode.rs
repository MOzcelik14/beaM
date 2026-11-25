#![no_main]

use libfuzzer_sys::fuzz_target;
use rawler::imgop::develop::{ProcessingStep, RawDevelop};
use rawler::rawsource::RawSource;
use rawler::decoders::RawDecodeParams;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 || data.len() > 10 * 1024 * 1024 {
        return;
    }

    if beam_media::decoder::validate_tiff_raw_structure(data, data.len()).is_err() {
        return;
    }
    if beam_media::decoder::validate_raw_container_with_size(data, data.len()).is_err() {
        return;
    }

    let source = RawSource::new_from_slice(data);
    if let Ok(decoder) = rawler::get_decoder(&source) {
        if let Ok(meta_img) = decoder.raw_image(&source, &RawDecodeParams::default(), true) {
            let w = meta_img.width as u32;
            let h = meta_img.height as u32;
            if w == 0
                || h == 0
                || w > beam_media::decoder::MAX_IMAGE_DIMENSION
                || h > beam_media::decoder::MAX_IMAGE_DIMENSION
                || (w as u64) * (h as u64) > beam_media::decoder::MAX_IMAGE_PIXELS
            {
                return;
            }
        }
    }

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
});
