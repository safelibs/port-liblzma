///////////////////////////////////////////////////////////////////////////////
//
/// \file       test_file_info_decoder.c
/// \brief      Regression tests for lzma_file_info_decoder
//
//  This file has been put into the public domain.
//  You can do whatever you want with this file.
//
///////////////////////////////////////////////////////////////////////////////

#include "tests.h"

#include <stdio.h>
#include <stdlib.h>

#ifndef SAFE_TEST_FILES_DIR
#	error SAFE_TEST_FILES_DIR must be defined
#endif


static uint8_t *
read_test_file(const char *name, size_t *size)
{
	char path[512];
	assert_true(snprintf(path, sizeof(path), "%s/%s",
			SAFE_TEST_FILES_DIR, name) > 0);

	FILE *file = fopen(path, "rb");
	assert_true(file != NULL);

	assert_true(fseek(file, 0, SEEK_END) == 0);
	const long file_size = ftell(file);
	assert_true(file_size >= 0);
	assert_true(fseek(file, 0, SEEK_SET) == 0);

	*size = (size_t)file_size;
	uint8_t *buf = tuktest_malloc(*size);
	assert_uint_eq(fread(buf, 1, *size, file), *size);
	fclose(file);

	return buf;
}


static lzma_ret
run_file_info_decoder(const uint8_t *file, size_t file_size,
		size_t chunk_size, lzma_action initial_action,
		bool *saw_seek, lzma_index **dest_index)
{
	lzma_stream strm = LZMA_STREAM_INIT;
	*dest_index = NULL;
	*saw_seek = false;

	assert_lzma_ret(lzma_file_info_decoder(&strm, dest_index,
			UINT64_MAX, file_size), LZMA_OK);

	size_t pos = 0;
	lzma_action action = initial_action;
	lzma_ret ret;

	while (true) {
		size_t avail = pos < file_size ? file_size - pos : 0;
		if (chunk_size != 0 && avail > chunk_size)
			avail = chunk_size;

		strm.next_in = avail == 0 ? NULL : file + pos;
		strm.avail_in = avail;
		ret = lzma_code(&strm, action);
		pos += avail - strm.avail_in;

		if (ret == LZMA_OK) {
			if (avail == 0)
				ret = LZMA_BUF_ERROR;
			else
				continue;
		}

		if (ret == LZMA_BUF_ERROR)
			break;

		if (ret == LZMA_OK)
			continue;

		if (ret == LZMA_SEEK_NEEDED) {
			*saw_seek = true;
			assert_uint(strm.seek_pos, <=, file_size);
			pos = (size_t)strm.seek_pos;
			action = LZMA_RUN;
			continue;
		}

		break;
	}

	lzma_end(&strm);
	return ret;
}


static void
test_buffered_single_stream(void)
{
	size_t file_size = 0;
	uint8_t *file = read_test_file("good-1-check-crc32.xz", &file_size);
	assert_uint(file_size, >, 16);

	bool saw_seek = false;
	lzma_index *index = NULL;
	assert_lzma_ret(run_file_info_decoder(file, file_size, file_size,
			LZMA_FINISH, &saw_seek, &index), LZMA_STREAM_END);
	assert_false(saw_seek);
	assert_true(index != NULL);
	assert_uint_eq(lzma_index_stream_count(index), 1);
	assert_uint_eq(lzma_index_block_count(index), 1);
	assert_uint_eq(lzma_index_file_size(index), file_size);
	assert_uint(lzma_index_uncompressed_size(index), >, 0);
	assert_uint_eq(lzma_index_checks(index), UINT32_C(1) << LZMA_CHECK_CRC32);

	lzma_index_end(index, NULL);
	tuktest_free(file);
}


static void
test_seek_needed_small_chunks(void)
{
	size_t file_size = 0;
	uint8_t *file = read_test_file("good-1-delta-lzma2.tiff.xz", &file_size);

	bool saw_seek = false;
	lzma_index *index = NULL;
	assert_lzma_ret(run_file_info_decoder(file, file_size, 17,
			LZMA_RUN, &saw_seek, &index), LZMA_STREAM_END);
	assert_true(saw_seek);
	assert_true(index != NULL);
	assert_uint_eq(lzma_index_stream_count(index), 1);
	assert_uint_eq(lzma_index_block_count(index), 1);
	assert_uint_eq(lzma_index_file_size(index), file_size);

	lzma_index_end(index, NULL);
	tuktest_free(file);
}


static void
test_concatenated_streams(void)
{
	size_t file_size = 0;
	uint8_t *file = read_test_file("good-1-delta-lzma2.tiff.xz", &file_size);
	const size_t concat_size = file_size * 2;
	uint8_t *concat = tuktest_malloc(concat_size);
	memcpy(concat, file, file_size);
	memcpy(concat + file_size, file, file_size);

	bool saw_seek = false;
	lzma_index *index = NULL;
	assert_lzma_ret(run_file_info_decoder(concat, concat_size, 19,
			LZMA_RUN, &saw_seek, &index), LZMA_STREAM_END);
	assert_true(saw_seek);
	assert_true(index != NULL);
	assert_uint_eq(lzma_index_stream_count(index), 2);
	assert_uint_eq(lzma_index_block_count(index), 2);
	assert_uint_eq(lzma_index_file_size(index), concat_size);

	lzma_index_iter iter;
	lzma_index_iter_init(&iter, index);
	assert_false(lzma_index_iter_next(&iter, LZMA_INDEX_ITER_STREAM));
	assert_uint_eq(iter.stream.number, 1);
	assert_false(lzma_index_iter_next(&iter, LZMA_INDEX_ITER_STREAM));
	assert_uint_eq(iter.stream.number, 2);

	lzma_index_end(index, NULL);
	tuktest_free(concat);
	tuktest_free(file);
}


static void
test_invalid_short_and_truncated_inputs(void)
{
	static const uint8_t short_file[4] = { 0xFD, 0x37, 0x7A, 0x58 };
	bool saw_seek = false;
	lzma_index *index = NULL;
	assert_lzma_ret(run_file_info_decoder(short_file, sizeof(short_file),
			sizeof(short_file), LZMA_FINISH,
			&saw_seek, &index), LZMA_FORMAT_ERROR);
	assert_true(index == NULL);

	size_t file_size = 0;
	uint8_t *file = read_test_file("good-1-check-crc32.xz", &file_size);

	assert_lzma_ret(run_file_info_decoder(file, file_size - 1,
			file_size - 1, LZMA_FINISH,
			&saw_seek, &index), LZMA_DATA_ERROR);
	assert_true(index == NULL);

	const size_t truncated_index_size = file_size - 4;
	uint8_t *truncated_index = tuktest_malloc(truncated_index_size);
	memcpy(truncated_index, file, file_size - 16);
	memcpy(truncated_index + file_size - 16, file + file_size - 12, 12);

	assert_lzma_ret(run_file_info_decoder(truncated_index,
			truncated_index_size, truncated_index_size,
			LZMA_FINISH, &saw_seek, &index), LZMA_DATA_ERROR);
	assert_true(index == NULL);

	tuktest_free(truncated_index);
	tuktest_free(file);
}


int
main(int argc, char **argv)
{
	tuktest_start(argc, argv);
	tuktest_run(test_buffered_single_stream);
	tuktest_run(test_seek_needed_small_chunks);
	tuktest_run(test_concatenated_streams);
	tuktest_run(test_invalid_short_and_truncated_inputs);
	return tuktest_end();
}
