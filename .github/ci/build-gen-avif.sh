#!/bin/sh
# Builds the CI-only AVIF generator tools into /opt/gen, each pinned by tag AND commit
# (docs/corpus.md "Generated files"). Run once by the gen-avif stage of .github/ci/Dockerfile.
# Nothing here is a dependency of the crate, and nothing it makes is committed.
#
#   libaom v3.14.1   03087864cf4bea6abb0d28f95cf7843511413d8f   BSD-2-Clause + AOM patent licence
#                                                              (the AV1 encoder of avifenc; the tag libavif v1.4.2 names in ext/aom.cmd)
#   libavif v1.4.2   c5240fc79fe5c2407e10afd35f5505ef6333ea49   BSD-2-Clause (avifenc, avifgainmaputil)
#   libargparse      ee74d1b53bd680748af14e737378de57e2a0a954   MIT (fetched by libavif's own cmake at that commit)
#   libjpeg, libpng, libxml2, zlib: the Debian bookworm -dev packages (the apps read the JPEG and its gain-map XMP)
set -eu
prefix=/opt/gen
src=/opt/gen-src
build=/opt/gen-build
mkdir -p "$prefix" "$src" "$build"

# pin <dir> <url> <tag> <commit>: a shallow checkout of <commit>; the tag must point at it.
pin() {
  git init -q "$src/$1"
  (
    cd "$src/$1"
    git remote add origin "$2"
    git fetch -q --depth 1 origin "refs/tags/$3:refs/tags/$3"
    tag_commit=$(git rev-parse "$3^{commit}")
    [ "$tag_commit" = "$4" ] || { echo "build-gen-avif.sh: tag $3 of $2 is $tag_commit, not $4" >&2; exit 1; }
    git checkout -q "$3"
    [ "$(git rev-parse HEAD)" = "$4" ] || { echo "build-gen-avif.sh: $1 is not $4" >&2; exit 1; }
  )
}

pin libaom https://aomedia.googlesource.com/aom v3.14.1 03087864cf4bea6abb0d28f95cf7843511413d8f
pin libavif https://github.com/AOMediaCodec/libavif v1.4.2 c5240fc79fe5c2407e10afd35f5505ef6333ea49

# libaom: static, encoder and decoder, no tools, no tests.
cmake -S "$src/libaom" -B "$build/aom" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix" \
  -DCMAKE_INSTALL_LIBDIR=lib -DBUILD_SHARED_LIBS=OFF -DCONFIG_PIC=1 -DENABLE_DOCS=0 -DENABLE_EXAMPLES=0 \
  -DENABLE_TESTDATA=0 -DENABLE_TESTS=0 -DENABLE_TOOLS=0
cmake --build "$build/aom" -j "$(nproc)"
cmake --install "$build/aom"

# libavif with the apps: aom is the only codec (it encodes and decodes), the image libraries are the system ones.
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig"
cmake -S "$src/libavif" -B "$build/avif" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix" \
  -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_PREFIX_PATH="$prefix" -DBUILD_SHARED_LIBS=OFF \
  -DAVIF_CODEC_AOM=SYSTEM -DAVIF_CODEC_DAV1D=OFF -DAVIF_CODEC_LIBGAV1=OFF -DAVIF_CODEC_RAV1E=OFF -DAVIF_CODEC_SVT=OFF \
  -DAVIF_LIBYUV=OFF -DAVIF_LIBSHARPYUV=OFF -DAVIF_JPEG=SYSTEM -DAVIF_ZLIBPNG=SYSTEM -DAVIF_LIBXML2=SYSTEM \
  -DAVIF_BUILD_APPS=ON -DAVIF_BUILD_TESTS=OFF -DAVIF_BUILD_EXAMPLES=OFF -DAVIF_ENABLE_WERROR=OFF
cmake --build "$build/avif" -j "$(nproc)"
mkdir -p "$prefix/bin"
install "$build/avif/avifenc" "$build/avif/avifgainmaputil" "$prefix/bin/"
"$prefix/bin/avifenc" --version
"$prefix/bin/avifgainmaputil" --help >/dev/null
cat > "$prefix/versions-avif.txt" <<VEOF
Generators: libaom v3.14.1 (0308786), libavif v1.4.2 (c5240fc) avifenc and avifgainmaputil.
VEOF
rm -rf "$build" "$src"
