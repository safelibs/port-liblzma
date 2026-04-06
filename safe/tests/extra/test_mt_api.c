///////////////////////////////////////////////////////////////////////////////
//
/// \file       test_mt_api.c
/// \brief      Extra public API coverage for multithreaded stream coders
//
///////////////////////////////////////////////////////////////////////////////

#include "tests.h"

#include <stdlib.h>
#include <unistd.h>


#define MT_TIMEOUT_SAMPLE_SIZE (8U << 20)
#define MT_BLOCK_SIZE (64U << 10)


static void
fill_sample(uint8_t *buf, size_t size)
{
	uint32_t x = UINT32_C(0x12345678);
	for (size_t i = 0; i < size; ++i) {
		x = x * UINT32_C(1664525) + UINT32_C(1013904223);
		buf[i] = (uint8_t)(x >> 24);
	}
}


static lzma_mt
init_encoder_mt(uint32_t timeout)
{
	lzma_mt mt;
	memzero(&mt, sizeof(mt));
	mt.threads = 2;
	mt.block_size = MT_BLOCK_SIZE;
	mt.timeout = timeout;
	mt.preset = 6;
	mt.check = LZMA_CHECK_CRC32;
	return mt;
}


static void
assert_threaded_progress_visible(lzma_stream *strm)
{
	uint64_t progress_in = 0;
	uint64_t progress_out = 0;

	for (unsigned int i = 0; i < 100; ++i) {
		lzma_get_progress(strm, &progress_in, &progress_out);
		if (progress_in > 0 || progress_out > 0)
			break;
		usleep(1000);
	}

	assert_true(progress_in > 0 || progress_out > 0);
}


static void
test_stream_encoder_mt_rejects_null_options(void)
{
	lzma_stream enc = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_stream_encoder_mt(&enc, NULL), LZMA_PROG_ERROR);
	assert_uint_eq(lzma_stream_encoder_mt_memusage(NULL), UINT64_MAX);
}


static void
test_stream_encoder_mt_memusage_and_timeout_progress(void)
{
#if !TUKTEST_THREADS_ENABLED
	assert_skip("Threading support disabled");
#elif !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	if (!lzma_filter_encoder_is_supported(LZMA_FILTER_LZMA2)
			|| !lzma_filter_decoder_is_supported(
				LZMA_FILTER_LZMA2))
		assert_skip("LZMA2 encoder and/or decoder is disabled");

	uint8_t *input = tuktest_malloc(MT_TIMEOUT_SAMPLE_SIZE);
	fill_sample(input, MT_TIMEOUT_SAMPLE_SIZE);

	lzma_mt enc_mt = init_encoder_mt(1);
	const uint64_t memusage = lzma_stream_encoder_mt_memusage(&enc_mt);
	assert_uint(memusage, >, 0);
	assert_uint(memusage, <, UINT64_MAX);

	lzma_mt invalid_mt = enc_mt;
	invalid_mt.threads = 0;
	assert_uint_eq(lzma_stream_encoder_mt_memusage(&invalid_mt), UINT64_MAX);

	const size_t bound = lzma_stream_buffer_bound(MT_TIMEOUT_SAMPLE_SIZE);
	uint8_t *encoded = tuktest_malloc(bound);

	lzma_stream enc = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_stream_encoder_mt(&enc, &enc_mt), LZMA_OK);

	enc.next_in = input;
	enc.avail_in = MT_TIMEOUT_SAMPLE_SIZE;
	enc.next_out = encoded;
	enc.avail_out = bound;

	lzma_ret ret = lzma_code(&enc, LZMA_FINISH);
	assert_lzma_ret(ret, LZMA_OK);
	assert_uint(enc.avail_out, >, 0);
	assert_threaded_progress_visible(&enc);
	lzma_end(&enc);
	tuktest_free(encoded);
	tuktest_free(input);
#endif
}


int
main(int argc, char **argv)
{
	tuktest_start(argc, argv);

	tuktest_run(test_stream_encoder_mt_rejects_null_options);
	tuktest_run(test_stream_encoder_mt_memusage_and_timeout_progress);

	return tuktest_end();
}
