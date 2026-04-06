#include <stdint.h>
#include <stdio.h>

#include <lzma.h>

__attribute__((noinline))
static int helper(int input) {
  const uint32_t lzma_version = LZMA_VERSION;
  int local = input + 7;

  (void)lzma_version;
  puts("helper");
  return local * 3;
}

int main(void) {
  return helper(5) == 36 ? 0 : 1;
}
