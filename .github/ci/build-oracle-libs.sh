#!/bin/sh
# Builds the CI-only parity oracles from source, each pinned by tag AND commit (docs/parity.md),
# into /opt/oracle, and fetches ExifTool into /opt/exiftool. Run once by the oracle-libs stage of
# .github/ci/Dockerfile. Nothing here is a dependency of the crate.
#
#   libjpeg-turbo 3.2.0   c85e6b905bf237038faa936dab160ebfc5da0344   BSD-3-Clause, IJG, zlib
#   libultrahdr v2.0.2    e5f5a022fe96fc4dc2ee35c19f733a50df807abe   Apache-2.0 (libultrahdr's
#                                                                   gainmapmetadata.cpp: MIT OR Apache-2.0)
#   libavif v1.4.2        c5240fc79fe5c2407e10afd35f5505ef6333ea49   BSD-2-Clause
#   ExifTool 13.64        3d7a0538324e9b04aa8930201b007009491321d2   Perl licence (GPL or Artistic)
#
# libjpeg-turbo is built static with no SIMD (the output of the accurate integer DCT and of the
# fancy upsampling is the same with and without SIMD, and nasm is not needed). libultrahdr is
# built against it, with HEIF off. libavif is built with no codec: avifDecoderParse needs none.
set -eu
prefix=/opt/oracle
src=/opt/oracle-src
build=/opt/oracle-build
mkdir -p "$prefix" "$src" "$build"

# pin <dir> <url> <tag> <commit>: a shallow checkout of <commit>; the tag must point at it.
pin() {
  git init -q "$src/$1"
  (
    cd "$src/$1"
    git remote add origin "$2"
    git fetch -q --depth 1 origin "refs/tags/$3:refs/tags/$3"
    tag_commit=$(git rev-parse "$3^{commit}")
    [ "$tag_commit" = "$4" ] || { echo "build-oracle-libs.sh: tag $3 of $2 is $tag_commit, not $4" >&2; exit 1; }
    git checkout -q "$3"
    [ "$(git rev-parse HEAD)" = "$4" ] || { echo "build-oracle-libs.sh: $1 is not $4" >&2; exit 1; }
  )
}

pin libjpeg-turbo https://github.com/libjpeg-turbo/libjpeg-turbo 3.2.0 c85e6b905bf237038faa936dab160ebfc5da0344
pin libultrahdr https://github.com/google/libultrahdr v2.0.2 e5f5a022fe96fc4dc2ee35c19f733a50df807abe
pin libavif https://github.com/AOMediaCodec/libavif v1.4.2 c5240fc79fe5c2407e10afd35f5505ef6333ea49
pin exiftool https://github.com/exiftool/exiftool 13.64 3d7a0538324e9b04aa8930201b007009491321d2
rm -rf /opt/exiftool
mv "$src/exiftool" /opt/exiftool

# libjpeg-turbo: libjpeg.a (the API libultrahdr uses) and libturbojpeg.a (tj3 for the pixels).
cmake -S "$src/libjpeg-turbo" -B "$build/jpeg" -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib \
  -DENABLE_SHARED=OFF -DENABLE_STATIC=ON -DWITH_TURBOJPEG=ON -DWITH_SIMD=OFF \
  -DWITH_TOOLS=OFF -DWITH_TESTS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON
cmake --build "$build/jpeg" -j "$(nproc)"
cmake --install "$build/jpeg"

# libultrahdr: static, libjpeg-turbo from $prefix (not the bookworm libjpeg-dev), no HEIF, no
# sample app, no tests. The public header and the combined static library are copied by hand.
cmake -S "$src/libultrahdr" -B "$build/uhdr" -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF -DUHDR_BUILD_EXAMPLES=OFF -DUHDR_BUILD_TESTS=OFF -DUHDR_ENABLE_HEIF=OFF \
  -DUHDR_BUILD_DEPS=OFF -DUHDR_ENABLE_INSTALL=OFF -DCMAKE_PREFIX_PATH="$prefix" \
  -DJPEG_INCLUDE_DIR="$prefix/include" -DJPEG_LIBRARY="$prefix/lib/libjpeg.a"
cmake --build "$build/uhdr" -j "$(nproc)" --target uhdr
install -D "$(find "$build/uhdr" -maxdepth 1 -name 'libuhdr*.a' | head -n 1)" "$prefix/lib/libuhdr.a"
install -D "$src/libultrahdr/ultrahdr_api.h" "$prefix/include/ultrahdr_api.h"

# libavif: static, no codec, no apps.
cmake -S "$src/libavif" -B "$build/avif" -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib -DBUILD_SHARED_LIBS=OFF \
  -DAVIF_CODEC_AOM=OFF -DAVIF_CODEC_DAV1D=OFF -DAVIF_CODEC_LIBGAV1=OFF -DAVIF_CODEC_RAV1E=OFF \
  -DAVIF_CODEC_SVT=OFF -DAVIF_LIBYUV=OFF -DAVIF_LIBSHARPYUV=OFF -DAVIF_JPEG=OFF -DAVIF_ZLIBPNG=OFF \
  -DAVIF_BUILD_APPS=OFF -DAVIF_BUILD_TESTS=OFF -DAVIF_BUILD_EXAMPLES=OFF
cmake --build "$build/avif" -j "$(nproc)"
cmake --install "$build/avif"

ls -l "$prefix/lib" "$prefix/include"
cat > "$prefix/versions.txt" <<EOF
Oracles: libjpeg-turbo 3.2.0 (c85e6b9), libultrahdr v2.0.2 (e5f5a02), libavif v1.4.2 (c5240fc), ExifTool 13.64 (3d7a053).
EOF
rm -rf "$build" "$src"
