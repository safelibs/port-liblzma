///////////////////////////////////////////////////////////////////////////////
//
/// \file       tests.h
/// \brief      Common definitions for test applications
//
//  Author:     Lasse Collin
//
//  This file has been put into the public domain.
//  You can do whatever you want with this file.
//
///////////////////////////////////////////////////////////////////////////////

#ifndef LZMA_TESTS_H
#define LZMA_TESTS_H

#ifdef HAVE_CONFIG_H
#	include <config.h>
#endif

#include <stdbool.h>
#include <string.h>

#include "lzma.h"
#include "tuktest.h"


#define memzero(s, n) memset(s, 0, n)

#ifndef ARRAY_SIZE
#	define ARRAY_SIZE(array) (sizeof(array) / sizeof((array)[0]))
#endif


static inline uint32_t
read32le(const uint8_t *buf)
{
	return (uint32_t)buf[0]
			| ((uint32_t)buf[1] << 8)
			| ((uint32_t)buf[2] << 16)
			| ((uint32_t)buf[3] << 24);
}


static inline void
write32le(uint8_t *buf, uint32_t num)
{
	buf[0] = (uint8_t)(num);
	buf[1] = (uint8_t)(num >> 8);
	buf[2] = (uint8_t)(num >> 16);
	buf[3] = (uint8_t)(num >> 24);
}


#if defined(MYTHREAD_POSIX) || defined(MYTHREAD_WIN95) \
		|| defined(MYTHREAD_VISTA)
#	define TUKTEST_THREADS_ENABLED 1
#else
#	define TUKTEST_THREADS_ENABLED 0
#endif


// These come from the .xz file format, not liblzma's internal headers.
#define TUKTEST_UNPADDED_SIZE_MIN LZMA_VLI_C(5)
#define TUKTEST_UNPADDED_SIZE_MAX (LZMA_VLI_MAX & ~LZMA_VLI_C(3))
#define TUKTEST_INDEX_INDICATOR 0


static inline lzma_vli
tuktest_vli_ceil4(lzma_vli vli)
{
	return (vli + 3) & ~LZMA_VLI_C(3);
}


// Invalid value for the lzma_check enumeration. This must be positive
// but small enough to fit into signed char since the underlying type might
// one some platform be a signed char.
//
// Don't put LZMA_ at the beginning of the name so that it is obvious that
// this constant doesn't come from the API headers.
#define INVALID_LZMA_CHECK_ID ((lzma_check)(LZMA_CHECK_ID_MAX + 1))


// This table and macro allow getting more readable error messages when
// comparing the lzma_ret enumeration values.
static const char enum_strings_lzma_ret[][24] = {
	"LZMA_OK",
	"LZMA_STREAM_END",
	"LZMA_NO_CHECK",
	"LZMA_UNSUPPORTED_CHECK",
	"LZMA_GET_CHECK",
	"LZMA_MEM_ERROR",
	"LZMA_MEMLIMIT_ERROR",
	"LZMA_FORMAT_ERROR",
	"LZMA_OPTIONS_ERROR",
	"LZMA_DATA_ERROR",
	"LZMA_BUF_ERROR",
	"LZMA_PROG_ERROR",
	"LZMA_SEEK_NEEDED",
};

#define assert_lzma_ret(test_expr, ref_val) \
	assert_enum_eq(test_expr, ref_val, enum_strings_lzma_ret)


static const char enum_strings_lzma_check[][24] = {
	"LZMA_CHECK_NONE",
	"LZMA_CHECK_CRC32",
	"LZMA_CHECK_UNKNOWN_2",
	"LZMA_CHECK_UNKNOWN_3",
	"LZMA_CHECK_CRC64",
	"LZMA_CHECK_UNKNOWN_5",
	"LZMA_CHECK_UNKNOWN_6",
	"LZMA_CHECK_UNKNOWN_7",
	"LZMA_CHECK_UNKNOWN_8",
	"LZMA_CHECK_UNKNOWN_9",
	"LZMA_CHECK_SHA256",
	"LZMA_CHECK_UNKNOWN_11",
	"LZMA_CHECK_UNKNOWN_12",
	"LZMA_CHECK_UNKNOWN_13",
	"LZMA_CHECK_UNKNOWN_14",
	"LZMA_CHECK_UNKNOWN_15",
};

#define assert_lzma_check(test_expr, ref_val) \
	assert_enum_eq(test_expr, ref_val, enum_strings_lzma_check)

#endif
