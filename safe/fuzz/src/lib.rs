use safe_liblzma::ffi::{
    stubs::{lzma_code, lzma_end, lzma_stream_decoder},
    types::{lzma_stream, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_INIT},
};

const LZMA_CONCATENATED: u32 = 0x08;
const LZMA_IGNORE_CHECK: u32 = 0x10;
const LZMA_FINISH: i32 = 3;
const MEMLIMIT: u64 = 300 << 20;
const OUTPUT_SIZE: usize = 4096;

pub fn decode_one_input(input: &[u8]) {
    let mut outbuf = [0u8; OUTPUT_SIZE];
    let mut strm: lzma_stream = LZMA_STREAM_INIT;

    let ret =
        unsafe { lzma_stream_decoder(&mut strm, MEMLIMIT, LZMA_CONCATENATED | LZMA_IGNORE_CHECK) };
    assert_eq!(ret, LZMA_OK, "lzma_stream_decoder failed with {ret}");

    strm.next_in = input.as_ptr();
    strm.avail_in = input.len();
    strm.next_out = outbuf.as_mut_ptr();
    strm.avail_out = outbuf.len();

    loop {
        let ret = unsafe { lzma_code(&mut strm, LZMA_FINISH) };
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

    unsafe {
        lzma_end(&mut strm);
    }
}

#[no_mangle]
pub extern "C" fn LLVMFuzzerTestOneInput(data: *const u8, size: usize) -> i32 {
    let input = if size == 0 {
        &[]
    } else if data.is_null() {
        return 0;
    } else {
        unsafe { core::slice::from_raw_parts(data, size) }
    };

    decode_one_input(input);
    0
}
