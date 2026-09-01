#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_transport::{decode_frame, encode_frame, FrameFlags};
use tpt20_compat_grpc::{decode_grpc_frame, encode_grpc_frame};

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    // Fuzz native transport frame decode
    let _ = decode_frame(data);

    // Fuzz gRPC frame decode
    let _ = decode_grpc_frame(data);

    // Fuzz native frame encode with fuzz data as payload
    let _ = encode_frame(data, data[0] & 1 != 0);

    // Fuzz gRPC frame encode with fuzz data as payload
    if data.len() <= u32::MAX as usize {
        let _ = encode_grpc_frame(data, data[0] & 1 != 0);
    }

    // Fuzz FrameFlags::from_raw
    if !data.is_empty() {
        let _ = FrameFlags::from_raw(data[0]);
    }
});
