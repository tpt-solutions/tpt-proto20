#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_descriptor::Descriptor;

fuzz_target!(|data: &[u8]| {
    let _ = Descriptor::from_binary(data);
});
