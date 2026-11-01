#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = moxcms::ColorProfile::new_from_slice(data);
});
