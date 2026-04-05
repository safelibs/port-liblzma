///////////////////////////////////////////////////////////////////////////////
//
/// \file       test_microlzma.c
/// \brief      Regression tests for the public MicroLZMA APIs
//
///////////////////////////////////////////////////////////////////////////////

#include "tests.h"


#define SAMPLE_SIZE 64
#define ENCODE_CAPACITY 256


static void
fill_sample(uint8_t *buf, size_t size)
{
	for (size_t i = 0; i < size; ++i)
		buf[i] = (uint8_t)('A' + (i % 23));
}


static lzma_options_lzma
init_options(void)
{
	lzma_options_lzma opt;
	memzero(&opt, sizeof(opt));
	assert_false(lzma_lzma_preset(&opt, 1));
	opt.dict_size = 1U << 15;
	return opt;
}


static size_t
encode_microlzma(const uint8_t *in, size_t in_size,
		const lzma_options_lzma *opt,
		uint8_t *out, size_t out_size)
{
	lzma_stream strm = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_microlzma_encoder(&strm, opt), LZMA_OK);

	strm.next_in = in;
	strm.avail_in = in_size;
	strm.next_out = out;
	strm.avail_out = out_size;

	assert_lzma_ret(lzma_code(&strm, LZMA_FINISH), LZMA_STREAM_END);
	assert_uint_eq(strm.total_in, in_size);

	const size_t produced = (size_t)strm.total_out;
	lzma_end(&strm);
	return produced;
}


static void
test_roundtrip(void)
{
	if (!lzma_filter_encoder_is_supported(LZMA_FILTER_LZMA1)
			|| !lzma_filter_decoder_is_supported(
				LZMA_FILTER_LZMA1))
		assert_skip("LZMA1 encoder and/or decoder is disabled");

	const lzma_options_lzma opt = init_options();

	uint8_t input[SAMPLE_SIZE];
	fill_sample(input, sizeof(input));

	uint8_t encoded[ENCODE_CAPACITY];
	const size_t encoded_size = encode_microlzma(input, sizeof(input),
			&opt, encoded, sizeof(encoded));
	assert_uint(encoded_size, >, 0);

	lzma_stream strm = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_microlzma_decoder(&strm, encoded_size,
			sizeof(input), true, opt.dict_size), LZMA_OK);

	uint8_t decoded[SAMPLE_SIZE];
	memzero(decoded, sizeof(decoded));

	strm.next_in = encoded;
	strm.avail_in = encoded_size;
	strm.next_out = decoded;
	strm.avail_out = sizeof(decoded);

	assert_lzma_ret(lzma_code(&strm, LZMA_FINISH), LZMA_STREAM_END);
	assert_uint_eq(strm.total_in, encoded_size);
	assert_uint_eq(strm.total_out, sizeof(input));
	assert_array_eq(decoded, input, sizeof(input));

	lzma_end(&strm);
}


static void
test_memlimit_controls(void)
{
	const lzma_options_lzma opt = init_options();

	uint8_t input[SAMPLE_SIZE];
	fill_sample(input, sizeof(input));

	uint8_t encoded[ENCODE_CAPACITY];
	const size_t encoded_size = encode_microlzma(input, sizeof(input),
			&opt, encoded, sizeof(encoded));

	lzma_stream strm = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_microlzma_decoder(&strm, encoded_size,
			sizeof(input), true, opt.dict_size), LZMA_OK);

	assert_uint_eq(lzma_memusage(&strm), 0);
	assert_uint_eq(lzma_memlimit_get(&strm), 0);
	assert_lzma_ret(lzma_memlimit_set(&strm, 1), LZMA_PROG_ERROR);

	lzma_end(&strm);
}


static void
test_truncated_input(void)
{
	const lzma_options_lzma opt = init_options();

	uint8_t input[SAMPLE_SIZE];
	fill_sample(input, sizeof(input));

	uint8_t encoded[ENCODE_CAPACITY];
	const size_t encoded_size = encode_microlzma(input, sizeof(input),
			&opt, encoded, sizeof(encoded));

	lzma_stream strm = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_microlzma_decoder(&strm, encoded_size - 1,
			sizeof(input), true, opt.dict_size), LZMA_OK);

	uint8_t decoded[SAMPLE_SIZE];
	memzero(decoded, sizeof(decoded));

	strm.next_in = encoded;
	strm.avail_in = encoded_size - 1;
	strm.next_out = decoded;
	strm.avail_out = sizeof(decoded);

	lzma_ret ret = LZMA_OK;
	for (unsigned int i = 0; i < 8 && ret == LZMA_OK; ++i) {
		strm.next_out = decoded + strm.total_out;
		strm.avail_out = sizeof(decoded) - (size_t)strm.total_out;
		ret = lzma_code(&strm, LZMA_FINISH);
	}

	assert_lzma_ret(ret, LZMA_BUF_ERROR);
	assert_uint(strm.total_out, <, sizeof(input));
	lzma_end(&strm);
}


static void
test_zero_length_and_small_buffers(void)
{
	const lzma_options_lzma opt = init_options();

	uint8_t sample[SAMPLE_SIZE];
	fill_sample(sample, sizeof(sample));

	lzma_stream enc = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_microlzma_encoder(&enc, &opt), LZMA_OK);

	uint8_t too_small[5];
	enc.next_in = sample;
	enc.avail_in = sizeof(sample);
	enc.next_out = too_small;
	enc.avail_out = sizeof(too_small);
	assert_lzma_ret(lzma_code(&enc, LZMA_FINISH), LZMA_PROG_ERROR);
	assert_uint_eq(enc.total_in, 0);
	assert_uint_eq(enc.total_out, 0);
	lzma_end(&enc);

	uint8_t encoded_empty[ENCODE_CAPACITY];
	const size_t encoded_empty_size = encode_microlzma(NULL, 0, &opt,
			encoded_empty, sizeof(encoded_empty));
	assert_uint(encoded_empty_size, >, 0);

	lzma_stream dec = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_microlzma_decoder(&dec, encoded_empty_size,
			0, true, opt.dict_size), LZMA_OK);
	dec.next_in = encoded_empty;
	dec.avail_in = encoded_empty_size;
	dec.next_out = NULL;
	dec.avail_out = 0;
	assert_lzma_ret(lzma_code(&dec, LZMA_FINISH), LZMA_STREAM_END);
	assert_uint_eq(dec.total_out, 0);
	lzma_end(&dec);

	uint8_t encoded[SAMPLE_SIZE * 4];
	const size_t encoded_size = encode_microlzma(sample, sizeof(sample),
			&opt, encoded, sizeof(encoded));

	dec = (lzma_stream)LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_microlzma_decoder(&dec, encoded_size,
			sizeof(sample) - 1, false, opt.dict_size), LZMA_OK);

	uint8_t partial[SAMPLE_SIZE - 1];
	memzero(partial, sizeof(partial));

	dec.next_in = encoded;
	dec.avail_in = encoded_size;
	dec.next_out = partial;
	dec.avail_out = sizeof(partial);
	assert_lzma_ret(lzma_code(&dec, LZMA_FINISH), LZMA_STREAM_END);
	assert_uint_eq(dec.total_out, sizeof(partial));
	assert_array_eq(partial, sample, sizeof(partial));
	lzma_end(&dec);
}


extern int
main(int argc, char **argv)
{
	tuktest_start(argc, argv);

	tuktest_run(test_roundtrip);
	tuktest_run(test_memlimit_controls);
	tuktest_run(test_truncated_input);
	tuktest_run(test_zero_length_and_small_buffers);

	return tuktest_end();
}
