use safe_liblzma::ffi::{
    stubs::{lzma_code, lzma_end, lzma_stream_decoder},
    types::{lzma_stream, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_INIT},
};

const LZMA_CONCATENATED: u32 = 0x08;
const LZMA_IGNORE_CHECK: u32 = 0x10;
const LZMA_FINISH: i32 = 3;
const MEMLIMIT: u64 = 300 << 20;
const OUTPUT_SIZE: usize = 4096;

fn stream_decoder(strm: &mut lzma_stream) -> i32 {
    // SAFETY: `strm` points to a live stack-allocated `lzma_stream` initialized with
    // `LZMA_STREAM_INIT`, which matches the public C ABI contract for this entrypoint.
    unsafe { lzma_stream_decoder(strm, MEMLIMIT, LZMA_CONCATENATED | LZMA_IGNORE_CHECK) }
}

fn code(strm: &mut lzma_stream) -> i32 {
    // SAFETY: `strm` remains exclusively owned by this harness loop and its input/output
    // buffer pointers are set from live Rust slices before every call.
    unsafe { lzma_code(strm, LZMA_FINISH) }
}

fn end(strm: &mut lzma_stream) {
    // SAFETY: `strm` was initialized through `lzma_stream_decoder` above and must be torn
    // down exactly once to release any decoder-owned state.
    unsafe {
        lzma_end(strm);
    }
}

pub fn decode_one_input(input: &[u8]) {
    let mut outbuf = [0u8; OUTPUT_SIZE];
    let mut strm: lzma_stream = LZMA_STREAM_INIT;

    let ret = stream_decoder(&mut strm);
    assert_eq!(ret, LZMA_OK, "lzma_stream_decoder failed with {ret}");

    strm.next_in = input.as_ptr();
    strm.avail_in = input.len();
    strm.next_out = outbuf.as_mut_ptr();
    strm.avail_out = outbuf.len();

    loop {
        let ret = code(&mut strm);
        if ret == LZMA_OK {
            if strm.avail_out == 0 {
                strm.next_out = outbuf.as_mut_ptr();
                strm.avail_out = outbuf.len();
            }
            continue;
        }

        assert_ne!(ret, LZMA_PROG_ERROR, "lzma_code returned LZMA_PROG_ERROR");
        break;
    }

    end(&mut strm);
}

#[no_mangle]
/// # Safety
///
/// `data` must be either null with `size == 0` or point to `size` readable bytes for the
/// duration of this callback, which matches libFuzzer's entrypoint contract.
pub unsafe extern "C" fn LLVMFuzzerTestOneInput(data: *const u8, size: usize) -> i32 {
    let input = if size == 0 {
        &[]
    } else if data.is_null() {
        return 0;
    } else {
        // SAFETY: libFuzzer provides a non-null pointer to `size` bytes for non-empty inputs,
        // and the harness does not retain the resulting slice beyond this callback.
        unsafe { core::slice::from_raw_parts(data, size) }
    };

    decode_one_input(input);
    0
}
