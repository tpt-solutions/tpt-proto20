#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_language::parse;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let src = String::from_utf8_lossy(data);
    let _ = parse(&src);
});
