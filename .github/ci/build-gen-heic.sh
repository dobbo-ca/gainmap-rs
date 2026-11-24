#!/bin/sh
# Builds the CI-only HEIC generator into /opt/gen: libultrahdr v2.0.2 with HEIF support, its sample
# app writing a HEIC with an ISO 21496-1 `tmap` item. Run once by the gen-heic stage of
# .github/ci/Dockerfile. Nothing here is a dependency of the crate, and nothing it makes is committed.
#
#   libultrahdr v2.0.2   e5f5a022fe96fc4dc2ee35c19f733a50df807abe   Apache-2.0
#   libheif 4a3f74bc     + libultrahdr's cmake/patches/libheif_pr1503.patch (the open libheif PR 1503: `tmap`
#                        writing; no released libheif has it); libultrahdr's own build fetches this commit
#   libjpeg-turbo        fetched and built by libultrahdr's own cmake (UHDR_BUILD_DEPS=ON)
#   x265                 Debian bookworm libx265-dev (3.5), GPL-2.0-or-later, the HEVC encoder of libheif; a CI tool only
set -eu
prefix=/opt/gen
src=/opt/gen-src
build=/opt/gen-build
mkdir -p "$prefix/bin" "$prefix/lib" "$src" "$build"

git init -q "$src/libultrahdr"
(
  cd "$src/libultrahdr"
  git remote add origin https://github.com/google/libultrahdr
  git fetch -q --depth 1 origin refs/tags/v2.0.2:refs/tags/v2.0.2
  [ "$(git rev-parse 'v2.0.2^{commit}')" = e5f5a022fe96fc4dc2ee35c19f733a50df807abe ] || { echo "build-gen-heic.sh: v2.0.2 moved" >&2; exit 1; }
  git checkout -q v2.0.2
)

# UHDR_WRITE_XMP=ON: the JPEGs also carry the hdrgm XMP (libavif needs it to read the gain map of a JPEG). libuhdr is shared here (the default), so the sample app links it dynamically; libheif is static inside
# it, and x265 is added to the link line by hand (libultrahdr's imported libheif target does not carry it).
cmake -S "$src/libultrahdr" -B "$build/uhdr" -DCMAKE_BUILD_TYPE=Release \
  -DUHDR_BUILD_DEPS=ON -DUHDR_WRITE_XMP=ON -DUHDR_ENABLE_HEIF=ON -DUHDR_BUILD_EXAMPLES=ON -DUHDR_BUILD_TESTS=OFF -DUHDR_ENABLE_INSTALL=OFF \
  -DCMAKE_CXX_STANDARD_LIBRARIES="-lx265 -ldl"
cmake --build "$build/uhdr" -j "$(nproc)"
install "$build/uhdr/ultrahdr_app" "$prefix/bin/"
find "$build/uhdr" -maxdepth 1 -name 'libuhdr.so*' -exec cp -a {} "$prefix/lib/" \;
# The raw 10-bit P010 input of the encoder (CC-BY-4.0, tests/data/LICENSE of that commit; docs/corpus.md).
mkdir -p "$prefix/data"
cp "$src/libultrahdr/tests/data/raw_p010_image.p010" "$prefix/data/"
LD_LIBRARY_PATH="$prefix/lib" "$prefix/bin/ultrahdr_app" 2>&1 | head -n 3 || true
echo "Generators: libultrahdr v2.0.2 (e5f5a02) with libheif 4a3f74bc + PR 1503 patch and Debian x265." > "$prefix/versions-heic.txt"
rm -rf "$build" "$src"
