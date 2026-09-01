#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_rpc::Metadata;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let key = format!("key-{}", data[0] % 10);
    let value = String::from_utf8_lossy(&data[1..]).to_string();

    let mut md = Metadata::new(1024);
    let _ = md.insert_text(key, value);
    let _ = md.insert_binary(format!("key-{}-bin", data[0] % 10), data);
});
