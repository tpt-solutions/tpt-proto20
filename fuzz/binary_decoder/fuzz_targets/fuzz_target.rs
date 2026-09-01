#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_core::{DecoderLimits, RawMessage, UnknownFieldPolicy};

fuzz_target!(|data: &[u8]| {
    let _ = RawMessage::decode(data, &DecoderLimits::default(), UnknownFieldPolicy::Preserve);
});
