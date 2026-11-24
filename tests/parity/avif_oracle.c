// CI-only parity oracle: libavif v1.4.2 with dav1d. Never built on a developer machine and never a
// dependency of the crate (docs/parity.md).
//
//   avif_oracle <in-dir> <out-dir>
//
// For every regular file <f> of <in-dir> (sorted) it writes <out-dir>/<f>.txt: the gain-map
// metadata that avifDecoderParse finds, as fractions, in the format of `oracle avif`
// (examples/oracle.rs). avifDecoderParse reads the container and the 'tmap' item and fills
// decoder->image->gainMap when imageContentToDecode has AVIF_IMAGE_CONTENT_GAIN_MAP.
//
//   status ok | no-gainmap | error
//   base.width N / base.height N / gainmap.width N / gainmap.height N
//   flags U                   U = useBaseColorSpace (0 or 1)
//   base_hdr_headroom n/d / alternate_hdr_headroom n/d
//   c.min n/d / c.max n/d / c.gamma n/d / c.base_offset n/d / c.alternate_offset n/d   (c = 0, 1, 2)
//
// "no-gainmap" is a file libavif parses without a usable gain map (none, an unsupported
// version, no 'tmap' brand, no 'altr' group, Adobe's auxiliary gain map, which libavif ignores).
// "error" is a parse failure.
//
// Where avifDecoderNextImage succeeds (imageContentToDecode has COLOR_AND_ALPHA and GAIN_MAP) it
// also writes the decoded planes, raw and in the format of `oracle planes` (examples/oracle.rs):
//
//   <f>.base.yuv, <f>.gm.yuv    Y, U, V planes (Y only for 4:0:0), tight rows, 1 byte per sample
//                               at depth 8, else 2 bytes little-endian
//   <f>.base.info, <f>.gm.info  "W H chroma depth depth" (chroma 400/420/422/444), then the
//                               transforms as "crop L T R B" ("crop inexact" when the clap is not
//                               an exact crop), "imir axis" and "irot degrees", in that order
//
// The gain-map file is written only when libavif found a gain map. It takes the transforms of the base image
// (libavif keeps none on the gain-map image and requires them to equal the base image's).
//
// This file is part of gainmap-rs (Apache-2.0). It calls the public API of libavif (BSD-2-Clause).

#include <dirent.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include <avif/avif.h>

static int cmp_names(const void * a, const void * b)
{
    return strcmp(*(const char * const *)a, *(const char * const *)b);
}

static uint8_t * read_file(const char * path, size_t * size)
{
    FILE * f = fopen(path, "rb");
    if (!f) return NULL;
    size_t cap = 1 << 20, len = 0;
    uint8_t * buf = malloc(cap);
    size_t n;
    while (buf && (n = fread(buf + len, 1, cap - len, f)) > 0) {
        len += n;
        if (len == cap) {
            cap *= 2;
            uint8_t * bigger = realloc(buf, cap);
            if (!bigger) {
                free(buf);
                buf = NULL;
            } else {
                buf = bigger;
            }
        }
    }
    fclose(f);
    *size = len;
    return buf;
}

/* The planes of `image` and the transforms of `tr` into <out>/<name>.<which>.yuv and .info. libavif keeps the
 * transforms on the decoded image only, and `gainMap->image` has none (read.c requires the gain-map item to have
 * the same clap, irot and imir as the base image), so the gain map takes the base image as `tr`. */
static void dump_image(const char * out, const char * name, const char * which, const avifImage * image, const avifImage * tr)
{
    char path[4096];
    avifPixelFormatInfo info;
    int chroma, bytes = image->depth > 8 ? 2 : 1;
    FILE * f;
    avifGetPixelFormatInfo(image->yuvFormat, &info);
    chroma = info.monochrome ? 400 : (info.chromaShiftX ? (info.chromaShiftY ? 420 : 422) : 444);

    snprintf(path, sizeof path, "%s/%s.%s.yuv", out, name, which);
    f = fopen(path, "wb");
    if (!f) return;
    for (int p = 0; p < (info.monochrome ? 1 : 3); ++p) {
        uint32_t w = p ? (image->width + (uint32_t)info.chromaShiftX) >> info.chromaShiftX : image->width;
        uint32_t h = p ? (image->height + (uint32_t)info.chromaShiftY) >> info.chromaShiftY : image->height;
        for (uint32_t y = 0; y < h; ++y) {
            const uint8_t * row = image->yuvPlanes[p] + (size_t)y * image->yuvRowBytes[p];
            if (bytes == 1) {
                fwrite(row, 1, w, f);
            } else {
                for (uint32_t x = 0; x < w; ++x) {
                    uint16_t v;
                    memcpy(&v, row + 2 * (size_t)x, 2);
                    fputc(v & 0xff, f);
                    fputc(v >> 8, f);
                }
            }
        }
    }
    fclose(f);

    snprintf(path, sizeof path, "%s/%s.%s.info", out, name, which);
    f = fopen(path, "wb");
    if (!f) return;
    fprintf(f, "%u %u %d %u %u\n", image->width, image->height, chroma, image->depth, image->depth);
    if (tr->transformFlags & AVIF_TRANSFORM_CLAP) {
        avifCropRect r;
        avifDiagnostics diag;
        memset(&diag, 0, sizeof diag);
        if (avifCropRectFromCleanApertureBox(&r, &tr->clap, image->width, image->height, &diag)) {
            fprintf(f, "crop %u %u %u %u\n", r.x, r.y, image->width - r.x - r.width, image->height - r.y - r.height);
        } else {
            fprintf(f, "crop inexact\n");
        }
    }
    if (tr->transformFlags & AVIF_TRANSFORM_IMIR) fprintf(f, "imir %u\n", tr->imir.axis);
    if (tr->transformFlags & AVIF_TRANSFORM_IROT) fprintf(f, "irot %u\n", tr->irot.angle * 90u);
    fclose(f);
}

static int write_text(const char * path, const char * text)
{
    FILE * f = fopen(path, "wb");
    if (!f) return 0;
    int ok = fputs(text, f) >= 0;
    return fclose(f) == 0 && ok;
}

// The text of one file, written into `text` (`cap` bytes).
static void describe(const uint8_t * data, size_t size, char * text, size_t cap, const char * out, const char * name)
{
    snprintf(text, cap, "status error\n");
    avifDecoder * decoder = avifDecoderCreate();
    if (!decoder) return;
    // Lenient, like the Rust parser: no pixi / clap / alpha ispe requirements.
    decoder->strictFlags = AVIF_STRICT_DISABLED;
    decoder->ignoreExif = AVIF_TRUE;
    decoder->ignoreXMP = AVIF_TRUE;
    decoder->imageContentToDecode = AVIF_IMAGE_CONTENT_COLOR_AND_ALPHA | AVIF_IMAGE_CONTENT_GAIN_MAP;
    const int parsed = avifDecoderSetIOMemory(decoder, data, size) == AVIF_RESULT_OK && avifDecoderParse(decoder) == AVIF_RESULT_OK;
    if (parsed) {
        const avifGainMap * gm = decoder->image ? decoder->image->gainMap : NULL;
        if (!gm || !gm->image) {
            snprintf(text, cap, "status no-gainmap\n");
        } else {
            size_t n = (size_t)snprintf(text,
                                        cap,
                                        "status ok\nbase.width %u\nbase.height %u\ngainmap.width %u\ngainmap.height %u\n"
                                        "flags %d\nbase_hdr_headroom %u/%u\nalternate_hdr_headroom %u/%u\n",
                                        decoder->image->width,
                                        decoder->image->height,
                                        gm->image->width,
                                        gm->image->height,
                                        gm->useBaseColorSpace ? 1 : 0,
                                        gm->baseHdrHeadroom.n,
                                        gm->baseHdrHeadroom.d,
                                        gm->alternateHdrHeadroom.n,
                                        gm->alternateHdrHeadroom.d);
            for (int c = 0; c < 3 && n < cap; ++c) {
                n += (size_t)snprintf(text + n,
                                      cap - n,
                                      "%d.min %d/%u\n%d.max %d/%u\n%d.gamma %u/%u\n%d.base_offset %d/%u\n%d.alternate_offset %d/%u\n",
                                      c, gm->gainMapMin[c].n, gm->gainMapMin[c].d,
                                      c, gm->gainMapMax[c].n, gm->gainMapMax[c].d,
                                      c, gm->gainMapGamma[c].n, gm->gainMapGamma[c].d,
                                      c, gm->baseOffset[c].n, gm->baseOffset[c].d,
                                      c, gm->alternateOffset[c].n, gm->alternateOffset[c].d);
            }
        }
    }
    // The planes, where libavif can decode them (dav1d). A failure writes nothing.
    if (parsed && avifDecoderNextImage(decoder) == AVIF_RESULT_OK && decoder->image && decoder->image->yuvPlanes[0]) {
        dump_image(out, name, "base", decoder->image, decoder->image);
        const avifGainMap * gm = decoder->image->gainMap;
        if (gm && gm->image && gm->image->yuvPlanes[0]) dump_image(out, name, "gm", gm->image, decoder->image);
    }
    avifDecoderDestroy(decoder);
}

int main(int argc, char ** argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: avif_oracle <in-dir> <out-dir>\n");
        return 2;
    }
    const char * in = argv[1];
    const char * out = argv[2];
    mkdir(out, 0777);
    DIR * dir = opendir(in);
    if (!dir) {
        fprintf(stderr, "avif_oracle: cannot open %s\n", in);
        return 1;
    }
    char ** names = NULL;
    size_t count = 0, cap = 0;
    struct dirent * e;
    while ((e = readdir(dir)) != NULL) {
        char path[4096];
        struct stat st;
        snprintf(path, sizeof path, "%s/%s", in, e->d_name);
        if (stat(path, &st) != 0 || !S_ISREG(st.st_mode)) continue; // stat follows symlinks
        if (count == cap) {
            cap = cap ? cap * 2 : 64;
            names = realloc(names, cap * sizeof *names);
            if (!names) return 1;
        }
        names[count++] = strdup(e->d_name);
    }
    closedir(dir);
    qsort(names, count, sizeof *names, cmp_names);

    char text[8192];
    for (size_t i = 0; i < count; ++i) {
        char path[4096];
        snprintf(path, sizeof path, "%s/%s", in, names[i]);
        size_t size = 0;
        uint8_t * data = read_file(path, &size);
        if (!data) {
            fprintf(stderr, "avif_oracle: cannot read %s\n", names[i]);
            return 1;
        }
        describe(data, size, text, sizeof text, out, names[i]);
        free(data);
        snprintf(path, sizeof path, "%s/%s.txt", out, names[i]);
        if (!write_text(path, text)) return 1;
        free(names[i]);
    }
    free(names);
    return 0;
}
