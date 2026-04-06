///////////////////////////////////////////////////////////////////////////////
//
/// \file       test_mt_regressions.c
/// \brief      Threaded decoder regression tests derived from Debian patches
//
///////////////////////////////////////////////////////////////////////////////

#include "tests.h"

#include <stdlib.h>


#define REGRESSION_SAMPLE_SIZE (1U << 20)


static void
fill_sample(uint8_t *buf, size_t size)
{
	uint32_t x = UINT32_C(0x9e3779b9);
	for (size_t i = 0; i < size; ++i) {
		x ^= x << 13;
		x ^= x >> 17;
		x ^= x << 5;
		buf[i] = (uint8_t)x;
	}
}


static lzma_mt
init_decoder_mt(void)
{
	lzma_mt mt;
	memzero(&mt, sizeof(mt));
	mt.threads = 2;
	mt.timeout = 0;
	mt.memlimit_threading = UINT64_C(1) << 28;
	mt.memlimit_stop = UINT64_C(1) << 28;
	return mt;
}


static void
make_streams(uint8_t **input_out, uint8_t **valid_out, size_t *valid_size_out,
		uint8_t **corrupt_out, size_t *corrupt_size_out)
{
	uint8_t *input = tuktest_malloc(REGRESSION_SAMPLE_SIZE);
	fill_sample(input, REGRESSION_SAMPLE_SIZE);

	lzma_options_lzma options;
	memzero(&options, sizeof(options));
	assert_false(lzma_lzma_preset(&options, LZMA_PRESET_DEFAULT));

	lzma_filter filters[LZMA_FILTERS_MAX + 1];
	memzero(filters, sizeof(filters));
	filters[0].id = LZMA_FILTER_LZMA2;
	filters[0].options = &options;
	filters[1].id = LZMA_VLI_UNKNOWN;

	const size_t bound = lzma_stream_buffer_bound(REGRESSION_SAMPLE_SIZE);
	uint8_t *valid = tuktest_malloc(bound);
	size_t valid_size = 0;
	assert_lzma_ret(lzma_stream_buffer_encode(filters, LZMA_CHECK_CRC32,
			NULL, input, REGRESSION_SAMPLE_SIZE,
			valid, &valid_size, bound), LZMA_OK);

	uint8_t *corrupt = tuktest_malloc(valid_size);
	memcpy(corrupt, valid, valid_size);
	const size_t payload_pos = LZMA_STREAM_HEADER_SIZE
			+ ((size_t)corrupt[LZMA_STREAM_HEADER_SIZE] + 1) * 4;
	assert_uint(payload_pos, <, valid_size);
	corrupt[payload_pos] = 0x03;

	*input_out = input;
	*valid_out = valid;
	*valid_size_out = valid_size;
	*corrupt_out = corrupt;
	*corrupt_size_out = valid_size;
}


static lzma_ret
decode_in_chunks(lzma_stream *strm, const uint8_t *in, size_t in_size,
		size_t chunk_size, size_t *consumed_out)
{
	uint8_t outbuf[4096];
	size_t pos = 0;

	for (unsigned int i = 0; i < 16384; ++i) {
		size_t avail = 0;
		if (pos < in_size) {
			avail = in_size - pos;
			if (avail > chunk_size)
				avail = chunk_size;
		}

		strm->next_in = avail == 0 ? NULL : in + pos;
		strm->avail_in = avail;
		strm->next_out = outbuf;
		strm->avail_out = sizeof(outbuf);

		const lzma_ret ret = lzma_code(strm, LZMA_FINISH);
		pos += avail - strm->avail_in;

		if (ret != LZMA_OK) {
			*consumed_out = pos;
			return ret;
		}
	}

	*consumed_out = pos;
	return LZMA_PROG_ERROR;
}


static void
test_error_while_main_thread_is_still_filling_worker_input(void)
{
#if !TUKTEST_THREADS_ENABLED
	assert_skip("Threading support disabled");
#elif !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	uint8_t *input = NULL;
	uint8_t *valid = NULL;
	uint8_t *corrupt = NULL;
	size_t valid_size = 0;
	size_t corrupt_size = 0;
	make_streams(&input, &valid, &valid_size, &corrupt, &corrupt_size);

	lzma_stream strm = LZMA_STREAM_INIT;
	const lzma_mt mt = init_decoder_mt();
	assert_lzma_ret(lzma_stream_decoder_mt(&strm, &mt), LZMA_OK);

	size_t consumed = 0;
	const lzma_ret ret = decode_in_chunks(&strm, corrupt, corrupt_size, 64,
			&consumed);
	assert_true(ret != LZMA_OK && ret != LZMA_STREAM_END);
	assert_uint(consumed, <, corrupt_size);

	lzma_end(&strm);
	tuktest_free(corrupt);
	tuktest_free(valid);
	tuktest_free(input);
#endif
}


static void
test_decoder_reinit_after_threaded_error(void)
{
#if !TUKTEST_THREADS_ENABLED
	assert_skip("Threading support disabled");
#elif !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	uint8_t *input = NULL;
	uint8_t *valid = NULL;
	uint8_t *corrupt = NULL;
	size_t valid_size = 0;
	size_t corrupt_size = 0;
	make_streams(&input, &valid, &valid_size, &corrupt, &corrupt_size);

	lzma_stream strm = LZMA_STREAM_INIT;
	const lzma_mt mt = init_decoder_mt();

	assert_lzma_ret(lzma_stream_decoder_mt(&strm, &mt), LZMA_OK);
	size_t consumed = 0;
	const lzma_ret ret = decode_in_chunks(&strm, corrupt, corrupt_size, 79,
			&consumed);
	assert_true(ret != LZMA_OK && ret != LZMA_STREAM_END);

	assert_lzma_ret(lzma_stream_decoder_mt(&strm, &mt), LZMA_OK);
	uint8_t *decoded = tuktest_malloc(REGRESSION_SAMPLE_SIZE);
	strm.next_in = valid;
	strm.avail_in = valid_size;
	strm.next_out = decoded;
	strm.avail_out = REGRESSION_SAMPLE_SIZE;
	assert_lzma_ret(lzma_code(&strm, LZMA_FINISH), LZMA_STREAM_END);
	assert_array_eq(decoded, input, REGRESSION_SAMPLE_SIZE);

	lzma_end(&strm);
	tuktest_free(decoded);
	tuktest_free(corrupt);
	tuktest_free(valid);
	tuktest_free(input);
#endif
}


static void
test_repeated_corrupt_threaded_decodes(void)
{
#if !TUKTEST_THREADS_ENABLED
	assert_skip("Threading support disabled");
#elif !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	uint8_t *input = NULL;
	uint8_t *valid = NULL;
	uint8_t *corrupt = NULL;
	size_t valid_size = 0;
	size_t corrupt_size = 0;
	make_streams(&input, &valid, &valid_size, &corrupt, &corrupt_size);

	const lzma_mt mt = init_decoder_mt();
	for (unsigned int i = 0; i < 16; ++i) {
		lzma_stream strm = LZMA_STREAM_INIT;
		assert_lzma_ret(lzma_stream_decoder_mt(&strm, &mt), LZMA_OK);

		size_t consumed = 0;
		const size_t chunk_size = 17 + i * 7;
		const lzma_ret ret = decode_in_chunks(&strm, corrupt, corrupt_size,
				chunk_size, &consumed);
		assert_true(ret != LZMA_OK && ret != LZMA_STREAM_END);
		assert_uint(consumed, <, corrupt_size);
		lzma_end(&strm);
	}

	tuktest_free(corrupt);
	tuktest_free(valid);
	tuktest_free(input);
#endif
}


int
main(int argc, char **argv)
{
	tuktest_start(argc, argv);

	tuktest_run(test_error_while_main_thread_is_still_filling_worker_input);
	tuktest_run(test_decoder_reinit_after_threaded_error);
	tuktest_run(test_repeated_corrupt_threaded_decodes);

	return tuktest_end();
}
