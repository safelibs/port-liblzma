#include <boost/iostreams/close.hpp>
#include <boost/iostreams/filter/lzma.hpp>
#include <boost/iostreams/filtering_stream.hpp>

#include <lzma.h>

#include <iostream>
#include <sstream>
#include <string>

namespace bio = boost::iostreams;

int main() {
  const unsigned int lzma_version = LZMA_VERSION;
  const std::string payload = std::string(4096, 'x') + " libboost-iostreams lzma";
  std::stringstream compressed;
  std::stringstream restored;

  (void)lzma_version;

  {
    bio::filtering_ostream out;
    out.push(bio::lzma_compressor());
    out.push(compressed);
    out << payload;
    bio::close(out);
  }

  {
    std::stringstream input(compressed.str());
    bio::filtering_istream in;
    in.push(bio::lzma_decompressor());
    in.push(input);
    restored << in.rdbuf();
  }

  if (restored.str() != payload) {
    return 1;
  }

  std::cout << "boost lzma ok\n";
  return 0;
}
