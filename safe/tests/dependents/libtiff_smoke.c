#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <lzma.h>
#include <tiffio.h>

int main(int argc, char **argv) {
  const char *path;
  const uint32_t width = 4;
  const uint32_t height = 4;
  const uint32_t lzma_version = LZMA_VERSION;
  uint16_t compression = 0;
  uint8_t rows[4][4] = {
    {0, 1, 2, 3},
    {4, 5, 6, 7},
    {8, 9, 10, 11},
    {12, 13, 14, 15},
  };

  (void)lzma_version;

  if (argc != 2) {
    fprintf(stderr, "usage: %s <output.tiff>\n", argv[0]);
    return 64;
  }

  path = argv[1];

  TIFF *out = TIFFOpen(path, "w");
  if (out == NULL) {
    return 1;
  }

  TIFFSetField(out, TIFFTAG_IMAGEWIDTH, width);
  TIFFSetField(out, TIFFTAG_IMAGELENGTH, height);
  TIFFSetField(out, TIFFTAG_SAMPLESPERPIXEL, 1);
  TIFFSetField(out, TIFFTAG_BITSPERSAMPLE, 8);
  TIFFSetField(out, TIFFTAG_ORIENTATION, ORIENTATION_TOPLEFT);
  TIFFSetField(out, TIFFTAG_PLANARCONFIG, PLANARCONFIG_CONTIG);
  TIFFSetField(out, TIFFTAG_PHOTOMETRIC, PHOTOMETRIC_MINISBLACK);
  TIFFSetField(out, TIFFTAG_COMPRESSION, COMPRESSION_LZMA);
  TIFFSetField(out, TIFFTAG_ROWSPERSTRIP, height);

  for (uint32_t row = 0; row < height; ++row) {
    if (TIFFWriteScanline(out, rows[row], row, 0) != 1) {
      TIFFClose(out);
      return 2;
    }
  }

  TIFFClose(out);

  TIFF *in = TIFFOpen(path, "r");
  if (in == NULL) {
    return 3;
  }

  TIFFGetField(in, TIFFTAG_COMPRESSION, &compression);
  if (compression != COMPRESSION_LZMA) {
    TIFFClose(in);
    return 4;
  }

  for (uint32_t row = 0; row < height; ++row) {
    uint8_t scanline[4];
    if (TIFFReadScanline(in, scanline, row, 0) != 1) {
      TIFFClose(in);
      return 5;
    }
    if (memcmp(scanline, rows[row], sizeof(scanline)) != 0) {
      TIFFClose(in);
      return 6;
    }
  }

  TIFFClose(in);
  puts("libtiff lzma ok");
  return 0;
}
