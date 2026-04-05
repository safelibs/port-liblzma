///////////////////////////////////////////////////////////////////////////////
//
/// \file       test_public_api.c
/// \brief      Tests uncovered public liblzma APIs
//
//  Author:     OpenAI
//
//  This file has been put into the public domain.
//  You can do whatever you want with this file.
//
///////////////////////////////////////////////////////////////////////////////

#include "tests.h"


#define SAMPLE_SIZE 513
#define STREAM_MEMLIMIT (UINT64_C(1) << 26)
#define ENCODE_BUFFER_SIZE (1U << 16)


static void
fill_sample(uint8_t *buf, size_t size)
{
	for (size_t i = 0; i < size; ++i)
		buf[i] = (uint8_t)(((i * 37) + (i >> 2)
				+ (i % 19) * 13) & 0xFF);
}


static void
init_lzma2_options(lzma_options_lzma *opt)
{
	assert_false(lzma_lzma_preset(opt, 1));
	opt->dict_size = UINT32_C(1) << 20;
}


static void
init_lzma2_filters(lzma_filter *filters, lzma_options_lzma *opt)
{
	filters[0].id = LZMA_FILTER_LZMA2;
	filters[0].options = opt;
	filters[1].id = LZMA_VLI_UNKNOWN;
	filters[1].options = NULL;
}


static void
encode_to_stream_end(lzma_stream *strm, const uint8_t *in, size_t in_size,
		uint8_t *out, size_t *out_pos, size_t out_size,
		lzma_action action)
{
	lzma_ret ret = LZMA_OK;

	strm->next_in = in;
	strm->avail_in = in_size;

	while (ret == LZMA_OK) {
		assert_uint(*out_pos, <, out_size);
		strm->next_out = out + *out_pos;
		strm->avail_out = out_size - *out_pos;
		const size_t avail_out_before = strm->avail_out;

		ret = lzma_code(strm, action);
		*out_pos += avail_out_before - strm->avail_out;
	}

	assert_lzma_ret(ret, LZMA_STREAM_END);
}


static void
decode_to_stream_end(lzma_stream *strm, const uint8_t *in, size_t in_size,
		uint8_t *out, size_t *out_pos, size_t out_size)
{
	lzma_ret ret = LZMA_OK;

	strm->next_in = in;
	strm->avail_in = in_size;

	while (ret == LZMA_OK) {
		strm->next_out = out + *out_pos;
		strm->avail_out = out_size - *out_pos;
		const size_t avail_out_before = strm->avail_out;

		ret = lzma_code(strm, LZMA_FINISH);
		*out_pos += avail_out_before - strm->avail_out;
	}

	assert_lzma_ret(ret, LZMA_STREAM_END);
}


static void
test_version_and_support_queries(void)
{
	assert_uint_eq(lzma_version_number(), LZMA_VERSION);
	assert_str_eq(lzma_version_string(), LZMA_VERSION_STRING);

	assert_false(lzma_mf_is_supported((lzma_match_finder)0));
	assert_false(lzma_mf_is_supported((lzma_match_finder)0x7F));
	assert_false(lzma_mode_is_supported((lzma_mode)0));
	assert_false(lzma_mode_is_supported((lzma_mode)99));

#ifdef HAVE_MF_HC3
	assert_true(lzma_mf_is_supported(LZMA_MF_HC3));
#endif
#ifdef HAVE_MF_HC4
	assert_true(lzma_mf_is_supported(LZMA_MF_HC4));
#endif
#ifdef HAVE_MF_BT2
	assert_true(lzma_mf_is_supported(LZMA_MF_BT2));
#endif
#ifdef HAVE_MF_BT3
	assert_true(lzma_mf_is_supported(LZMA_MF_BT3));
#endif
#ifdef HAVE_MF_BT4
	assert_true(lzma_mf_is_supported(LZMA_MF_BT4));
#endif

	if (lzma_filter_encoder_is_supported(LZMA_FILTER_LZMA2)) {
		assert_true(lzma_mode_is_supported(LZMA_MODE_FAST));
		assert_true(lzma_mode_is_supported(LZMA_MODE_NORMAL));
		assert_uint(lzma_easy_encoder_memusage(LZMA_PRESET_DEFAULT), >, 0);
	} else {
		assert_false(lzma_mode_is_supported(LZMA_MODE_FAST));
		assert_false(lzma_mode_is_supported(LZMA_MODE_NORMAL));
		assert_uint_eq(lzma_easy_encoder_memusage(LZMA_PRESET_DEFAULT),
				UINT64_MAX);
	}

	if (lzma_filter_decoder_is_supported(LZMA_FILTER_LZMA2))
		assert_uint(lzma_easy_decoder_memusage(LZMA_PRESET_DEFAULT), >, 0);
	else
		assert_uint_eq(lzma_easy_decoder_memusage(LZMA_PRESET_DEFAULT),
				UINT64_MAX);

	assert_uint_eq(lzma_easy_encoder_memusage(10), UINT32_MAX);
	assert_uint_eq(lzma_easy_decoder_memusage(10), UINT32_MAX);
}


static void
test_filter_helpers(void)
{
#if !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	if (!lzma_filter_encoder_is_supported(LZMA_FILTER_LZMA2)
			|| !lzma_filter_decoder_is_supported(
				LZMA_FILTER_LZMA2))
		assert_skip("LZMA2 encoder and/or decoder is disabled");

	lzma_options_lzma lzma2_opt;
	init_lzma2_options(&lzma2_opt);

	lzma_filter src[2];
	init_lzma2_filters(src, &lzma2_opt);

	uint32_t props_size = 0;
	assert_lzma_ret(lzma_properties_size(&props_size, &src[0]), LZMA_OK);
	assert_uint_eq(props_size, 1);

	uint8_t props[1];
	assert_lzma_ret(lzma_properties_encode(&src[0], props), LZMA_OK);

	lzma_filter decoded[2] = {
		{ .id = LZMA_FILTER_LZMA2, .options = NULL },
		{ .id = LZMA_VLI_UNKNOWN, .options = NULL },
	};
	assert_lzma_ret(lzma_properties_decode(&decoded[0], NULL,
			props, props_size), LZMA_OK);
	assert_true(decoded[0].options != NULL);
	assert_uint_eq(((lzma_options_lzma *)decoded[0].options)->dict_size,
			lzma2_opt.dict_size);
	lzma_filters_free(decoded, NULL);

	lzma_filter dest[LZMA_FILTERS_MAX + 1];
	memzero(dest, sizeof(dest));
	assert_lzma_ret(lzma_filters_copy(src, dest, NULL), LZMA_OK);
	assert_uint_eq(dest[0].id, LZMA_FILTER_LZMA2);
	assert_uint_eq(dest[1].id, LZMA_VLI_UNKNOWN);
	assert_true(dest[0].options != NULL);
	assert_true(dest[0].options != src[0].options);

	const uint32_t copied_dict_size
			= ((lzma_options_lzma *)dest[0].options)->dict_size;
	lzma2_opt.dict_size += 4096;
	assert_uint_eq(((lzma_options_lzma *)dest[0].options)->dict_size,
			copied_dict_size);
	lzma_filters_free(dest, NULL);
	assert_uint_eq(dest[0].id, LZMA_VLI_UNKNOWN);
	assert_true(dest[0].options == NULL);

	lzma_filter placeholder_src[2] = {
		{ .id = LZMA_VLI_C(0x123456), .options = NULL },
		{ .id = LZMA_VLI_UNKNOWN, .options = NULL },
	};
	memzero(dest, sizeof(dest));
	assert_lzma_ret(lzma_filters_copy(placeholder_src, dest, NULL),
			LZMA_OK);
	assert_uint_eq(dest[0].id, placeholder_src[0].id);
	assert_true(dest[0].options == NULL);
	assert_uint_eq(dest[1].id, LZMA_VLI_UNKNOWN);

	uint32_t dummy = 0;
	lzma_filter bad_src[2] = {
		{ .id = LZMA_VLI_C(0x123456), .options = &dummy },
		{ .id = LZMA_VLI_UNKNOWN, .options = NULL },
	};
	dest[0].id = LZMA_FILTER_LZMA2;
	dest[0].options = NULL;
	dest[1].id = LZMA_VLI_UNKNOWN;
	dest[1].options = NULL;
	assert_lzma_ret(lzma_filters_copy(bad_src, dest, NULL),
			LZMA_OPTIONS_ERROR);
	assert_uint_eq(dest[0].id, LZMA_FILTER_LZMA2);
	assert_true(dest[0].options == NULL);
#endif
}


static void
test_raw_helpers(void)
{
#if !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	if (!lzma_filter_encoder_is_supported(LZMA_FILTER_LZMA2)
			|| !lzma_filter_decoder_is_supported(
				LZMA_FILTER_LZMA2))
		assert_skip("LZMA2 encoder and/or decoder is disabled");

	lzma_options_lzma lzma2_opt;
	init_lzma2_options(&lzma2_opt);

	lzma_filter filters[2];
	init_lzma2_filters(filters, &lzma2_opt);

	lzma_filter invalid[1] = {
		{ .id = LZMA_VLI_UNKNOWN, .options = NULL },
	};

	assert_uint(lzma_raw_encoder_memusage(filters), >, 0);
	assert_uint(lzma_raw_decoder_memusage(filters), >, 0);
	assert_uint_eq(lzma_raw_encoder_memusage(invalid), UINT64_MAX);
	assert_uint_eq(lzma_raw_decoder_memusage(invalid), UINT64_MAX);

	uint8_t in[SAMPLE_SIZE];
	fill_sample(in, sizeof(in));

	uint8_t too_small[1];
	size_t too_small_pos = 0;
	assert_lzma_ret(lzma_raw_buffer_encode(filters, NULL,
			in, sizeof(in), too_small, &too_small_pos, sizeof(too_small)),
		LZMA_BUF_ERROR);
	assert_uint_eq(too_small_pos, 0);

	uint8_t encoded[ENCODE_BUFFER_SIZE];
	size_t encoded_size = 0;
	assert_lzma_ret(lzma_raw_buffer_encode(filters, NULL,
			in, sizeof(in), encoded, &encoded_size, sizeof(encoded)),
		LZMA_OK);
	assert_uint(encoded_size, >, 0);

	size_t in_pos = 0;
	size_t out_pos = 0;
	uint8_t out[SAMPLE_SIZE];
	assert_lzma_ret(lzma_raw_buffer_decode(filters, NULL,
			encoded, &in_pos, encoded_size, out, &out_pos, sizeof(out)),
		LZMA_OK);
	assert_uint_eq(in_pos, encoded_size);
	assert_uint_eq(out_pos, sizeof(in));
	assert_array_eq(out, in, sizeof(in));

	lzma_stream encoder = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_raw_encoder(&encoder, filters), LZMA_OK);

	size_t streamed_size = 0;
	uint8_t streamed[ENCODE_BUFFER_SIZE];
	encode_to_stream_end(&encoder, in, sizeof(in), streamed,
			&streamed_size, sizeof(streamed), LZMA_FINISH);
	lzma_end(&encoder);

	lzma_stream decoder = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_raw_decoder(&decoder, filters), LZMA_OK);

	size_t decoded_size = 0;
	uint8_t decoded[SAMPLE_SIZE];
	decode_to_stream_end(&decoder, streamed, streamed_size, decoded,
			&decoded_size, sizeof(decoded));
	lzma_end(&decoder);

	assert_uint_eq(decoded_size, sizeof(in));
	assert_array_eq(decoded, in, sizeof(in));
#endif
}


static void
test_stream_helpers(void)
{
#if !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	if (!lzma_filter_encoder_is_supported(LZMA_FILTER_LZMA2)
			|| !lzma_filter_decoder_is_supported(
				LZMA_FILTER_LZMA2))
		assert_skip("LZMA2 encoder and/or decoder is disabled");

	uint8_t in[SAMPLE_SIZE];
	fill_sample(in, sizeof(in));

	const size_t bound = lzma_stream_buffer_bound(sizeof(in));
	assert_uint(bound, >, 0);

	uint8_t *easy_encoded = tuktest_malloc(bound);
	size_t easy_encoded_size = 0;
	assert_lzma_ret(lzma_easy_buffer_encode(LZMA_PRESET_DEFAULT,
			LZMA_CHECK_CRC32, NULL, in, sizeof(in),
			easy_encoded, &easy_encoded_size, bound), LZMA_OK);
	assert_uint(easy_encoded_size, <=, bound);

	uint64_t memlimit = UINT64_MAX;
	size_t in_pos = 0;
	size_t out_pos = 0;
	uint8_t easy_decoded[SAMPLE_SIZE];
	assert_lzma_ret(lzma_stream_buffer_decode(&memlimit, 0, NULL,
			easy_encoded, &in_pos, easy_encoded_size,
			easy_decoded, &out_pos, sizeof(easy_decoded)), LZMA_OK);
	assert_uint_eq(in_pos, easy_encoded_size);
	assert_uint_eq(out_pos, sizeof(in));
	assert_array_eq(easy_decoded, in, sizeof(in));

	lzma_stream easy_encoder = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_easy_encoder(&easy_encoder,
			LZMA_PRESET_DEFAULT, LZMA_CHECK_CRC32), LZMA_OK);
	assert_uint_eq(lzma_memusage(&easy_encoder), 0);

	uint8_t streamed_easy[ENCODE_BUFFER_SIZE];
	size_t streamed_easy_size = 0;
	encode_to_stream_end(&easy_encoder, in, sizeof(in), streamed_easy,
			&streamed_easy_size, sizeof(streamed_easy), LZMA_FINISH);

	uint64_t progress_in = 0;
	uint64_t progress_out = 0;
	lzma_get_progress(&easy_encoder, &progress_in, &progress_out);
	assert_uint_eq(progress_in, easy_encoder.total_in);
	assert_uint_eq(progress_out, easy_encoder.total_out);
	assert_uint_eq(progress_in, sizeof(in));
	lzma_end(&easy_encoder);

	lzma_stream easy_decoder = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_stream_decoder(&easy_decoder,
			STREAM_MEMLIMIT, 0), LZMA_OK);
	assert_uint(lzma_memusage(&easy_decoder), >, 0);

	size_t streamed_decoded_size = 0;
	uint8_t streamed_decoded[SAMPLE_SIZE];
	decode_to_stream_end(&easy_decoder, streamed_easy, streamed_easy_size,
			streamed_decoded, &streamed_decoded_size,
			sizeof(streamed_decoded));
	lzma_get_progress(&easy_decoder, &progress_in, &progress_out);
	assert_uint_eq(progress_in, easy_decoder.total_in);
	assert_uint_eq(progress_out, easy_decoder.total_out);
	lzma_end(&easy_decoder);

	assert_uint_eq(streamed_decoded_size, sizeof(in));
	assert_array_eq(streamed_decoded, in, sizeof(in));

	lzma_options_lzma first_lzma2;
	init_lzma2_options(&first_lzma2);
	lzma_filter first_filters[2];
	init_lzma2_filters(first_filters, &first_lzma2);

	lzma_options_lzma second_lzma2;
	init_lzma2_options(&second_lzma2);
	second_lzma2.lc = 2;
	second_lzma2.lp = 1;
	second_lzma2.pb = 1;

	lzma_filter second_filters[LZMA_FILTERS_MAX + 1];
	memzero(second_filters, sizeof(second_filters));

	if (lzma_filter_encoder_is_supported(LZMA_FILTER_DELTA)
			&& lzma_filter_decoder_is_supported(LZMA_FILTER_DELTA)) {
		static lzma_options_delta delta = {
			.dist = 1
		};
		second_filters[0].id = LZMA_FILTER_DELTA;
		second_filters[0].options = &delta;
		second_filters[1].id = LZMA_FILTER_LZMA2;
		second_filters[1].options = &second_lzma2;
		second_filters[2].id = LZMA_VLI_UNKNOWN;
	} else {
		second_filters[0].id = LZMA_FILTER_LZMA2;
		second_filters[0].options = &second_lzma2;
		second_filters[1].id = LZMA_VLI_UNKNOWN;
	}

	const size_t first_size = sizeof(in) / 2;
	const size_t second_size = sizeof(in) - first_size;

	lzma_stream stream_encoder = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_stream_encoder(&stream_encoder,
			first_filters, LZMA_CHECK_CRC32), LZMA_OK);

	uint8_t multi_block[ENCODE_BUFFER_SIZE];
	size_t multi_block_size = 0;
	encode_to_stream_end(&stream_encoder, in, first_size,
			multi_block, &multi_block_size,
			sizeof(multi_block), LZMA_FULL_FLUSH);

	assert_lzma_ret(lzma_filters_update(&stream_encoder, second_filters),
			LZMA_OK);

	encode_to_stream_end(&stream_encoder, in + first_size, second_size,
			multi_block, &multi_block_size,
			sizeof(multi_block), LZMA_FINISH);
	lzma_end(&stream_encoder);

	memlimit = UINT64_MAX;
	in_pos = 0;
	out_pos = 0;
	uint8_t multi_block_decoded[SAMPLE_SIZE];
	assert_lzma_ret(lzma_stream_buffer_decode(&memlimit, 0, NULL,
			multi_block, &in_pos, multi_block_size,
			multi_block_decoded, &out_pos,
			sizeof(multi_block_decoded)), LZMA_OK);
	assert_uint_eq(in_pos, multi_block_size);
	assert_uint_eq(out_pos, sizeof(in));
	assert_array_eq(multi_block_decoded, in, sizeof(in));
#endif
}


static void
test_block_helpers(void)
{
#if !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	if (!lzma_filter_encoder_is_supported(LZMA_FILTER_LZMA2)
			|| !lzma_filter_decoder_is_supported(
				LZMA_FILTER_LZMA2))
		assert_skip("LZMA2 encoder and/or decoder is disabled");

	uint8_t in[SAMPLE_SIZE];
	fill_sample(in, sizeof(in));

	lzma_options_lzma lzma2_opt;
	init_lzma2_options(&lzma2_opt);

	lzma_filter filters[2];
	init_lzma2_filters(filters, &lzma2_opt);

	const size_t bound = lzma_block_buffer_bound(sizeof(in));
	assert_uint(bound, >, 0);

	uint8_t *encoded = tuktest_malloc(bound);
	size_t encoded_size = 0;
	lzma_block block = {
		.version = 1,
		.check = LZMA_CHECK_CRC32,
		.filters = filters,
	};
	assert_lzma_ret(lzma_block_buffer_encode(&block, NULL,
			in, sizeof(in), encoded, &encoded_size, bound), LZMA_OK);
	assert_uint_eq(block.uncompressed_size, sizeof(in));
	assert_uint_eq(lzma_block_total_size(&block), encoded_size);
	assert_uint(lzma_block_unpadded_size(&block), >, 0);
	assert_uint(lzma_block_unpadded_size(&block), <=, encoded_size);
	assert_uint_eq(encoded_size % 4, 0);

	lzma_filter decoded_filters[LZMA_FILTERS_MAX + 1];
	memzero(decoded_filters, sizeof(decoded_filters));

	lzma_block decoded_block;
	memzero(&decoded_block, sizeof(decoded_block));
	decoded_block.version = 1;
	decoded_block.check = block.check;
	decoded_block.header_size = lzma_block_header_size_decode(encoded[0]);
	decoded_block.filters = decoded_filters;

	assert_lzma_ret(lzma_block_header_decode(&decoded_block,
			NULL, encoded), LZMA_OK);
	assert_lzma_ret(lzma_block_compressed_size(&decoded_block,
			lzma_block_unpadded_size(&block)), LZMA_OK);
	assert_uint_eq(decoded_block.compressed_size, block.compressed_size);

	size_t in_pos = decoded_block.header_size;
	size_t out_pos = 0;
	uint8_t out[SAMPLE_SIZE];
	assert_lzma_ret(lzma_block_buffer_decode(&decoded_block, NULL,
			encoded, &in_pos, encoded_size, out, &out_pos, sizeof(out)),
		LZMA_OK);
	assert_uint_eq(in_pos, encoded_size);
	assert_uint_eq(out_pos, sizeof(in));
	assert_uint_eq(decoded_block.uncompressed_size, sizeof(in));
	assert_uint_eq(lzma_block_total_size(&decoded_block), encoded_size);
	assert_array_eq(out, in, sizeof(in));
	assert_array_eq(decoded_block.raw_check, block.raw_check,
			lzma_check_size(block.check));
	lzma_filters_free(decoded_filters, NULL);

	uint8_t *uncompressed = tuktest_malloc(bound);
	size_t uncompressed_size = 0;
	lzma_block uncompressed_block = {
		.version = 1,
		.check = LZMA_CHECK_CRC32,
		.filters = filters,
	};
	assert_lzma_ret(lzma_block_uncomp_encode(&uncompressed_block,
			in, sizeof(in), uncompressed, &uncompressed_size, bound),
		LZMA_OK);
	assert_uint_eq(lzma_block_total_size(&uncompressed_block),
			uncompressed_size);

	memzero(decoded_filters, sizeof(decoded_filters));
	memzero(&decoded_block, sizeof(decoded_block));
	decoded_block.version = 1;
	decoded_block.check = uncompressed_block.check;
	decoded_block.header_size = lzma_block_header_size_decode(
			uncompressed[0]);
	decoded_block.filters = decoded_filters;

	assert_lzma_ret(lzma_block_header_decode(&decoded_block,
			NULL, uncompressed), LZMA_OK);
	assert_lzma_ret(lzma_block_compressed_size(&decoded_block,
			lzma_block_unpadded_size(&uncompressed_block)), LZMA_OK);

	in_pos = decoded_block.header_size;
	out_pos = 0;
	assert_lzma_ret(lzma_block_buffer_decode(&decoded_block, NULL,
			uncompressed, &in_pos, uncompressed_size,
			out, &out_pos, sizeof(out)), LZMA_OK);
	assert_uint_eq(in_pos, uncompressed_size);
	assert_uint_eq(out_pos, sizeof(in));
	assert_array_eq(out, in, sizeof(in));
	lzma_filters_free(decoded_filters, NULL);
#endif
}


static void
test_alone_roundtrip(void)
{
#if !defined(HAVE_ENCODERS) || !defined(HAVE_DECODERS)
	assert_skip("Encoder or decoder support disabled");
#else
	if (!lzma_filter_encoder_is_supported(LZMA_FILTER_LZMA1)
			|| !lzma_filter_decoder_is_supported(
				LZMA_FILTER_LZMA1))
		assert_skip("LZMA1 encoder and/or decoder is disabled");

	uint8_t in[SAMPLE_SIZE];
	fill_sample(in, sizeof(in));

	lzma_options_lzma opt;
	assert_false(lzma_lzma_preset(&opt, 1));

	lzma_stream encoder = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_alone_encoder(&encoder, &opt), LZMA_OK);

	uint8_t encoded[ENCODE_BUFFER_SIZE];
	size_t encoded_size = 0;
	encode_to_stream_end(&encoder, in, sizeof(in), encoded,
			&encoded_size, sizeof(encoded), LZMA_FINISH);
	lzma_end(&encoder);

	lzma_stream decoder = LZMA_STREAM_INIT;
	assert_lzma_ret(lzma_alone_decoder(&decoder, STREAM_MEMLIMIT),
			LZMA_OK);
	assert_uint(lzma_memusage(&decoder), >, 0);

	size_t decoded_size = 0;
	uint8_t decoded[SAMPLE_SIZE];
	decode_to_stream_end(&decoder, encoded, encoded_size, decoded,
			&decoded_size, sizeof(decoded));
	lzma_end(&decoder);

	assert_uint_eq(decoded_size, sizeof(in));
	assert_array_eq(decoded, in, sizeof(in));
#endif
}


extern int
main(int argc, char **argv)
{
	tuktest_start(argc, argv);

	tuktest_run(test_version_and_support_queries);
	tuktest_run(test_filter_helpers);
	tuktest_run(test_raw_helpers);
	tuktest_run(test_stream_helpers);
	tuktest_run(test_block_helpers);
	tuktest_run(test_alone_roundtrip);

	return tuktest_end();
}
