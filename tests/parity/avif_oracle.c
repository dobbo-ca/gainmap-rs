// CI-only parity oracle: libavif v1.4.2, built with no codec. Never built on a developer
// machine and never a dependency of the crate (docs/parity.md).
//
//   avif_oracle <in-dir> <out-dir>
//
// For every regular file <f> of <in-dir> (sorted) it writes <out-dir>/<f>.txt: the gain-map
// metadata that avifDecoderParse finds, as fractions, in the format of `oracle avif`
// (examples/oracle.rs). avifDecoderParse reads the container and the 'tmap' item and fills
// decoder->image->gainMap when imageContentToDecode has AVIF_IMAGE_CONTENT_GAIN_MAP; a codec
// is needed only by avifDecoderNextImage (avifDecoderCreateCodecs in src/read.c), which is never
// called, so the build needs no AV1 decoder.
//
//   status ok | no-gainmap | error
//   base.width N / base.height N / gainmap.width N / gainmap.height N
//   flags U                   U = useBaseColorSpace (0 or 1)
//   base_hdr_headroom n/d / alternate_hdr_headroom n/d
//   c.min n/d / c.max n/d / c.gamma n/d / c.base_offset n/d / c.alternate_offset n/d   (c = 0, 1, 2)
//
// "no-gainmap" is a file libavif parses without a usable gain map (none, an unsupported
// version, no 'tmap' brand, no 'altr' group). "error" is a parse failure.
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

static int write_text(const char * path, const char * text)
{
    FILE * f = fopen(path, "wb");
    if (!f) return 0;
    int ok = fputs(text, f) >= 0;
    return fclose(f) == 0 && ok;
}

// The text of one file, written into `text` (`cap` bytes).
static void describe(const uint8_t * data, size_t size, char * text, size_t cap)
{
    snprintf(text, cap, "status error\n");
    avifDecoder * decoder = avifDecoderCreate();
    if (!decoder) return;
    // Lenient, like the Rust parser: no pixi / clap / alpha ispe requirements.
    decoder->strictFlags = AVIF_STRICT_DISABLED;
    decoder->ignoreExif = AVIF_TRUE;
    decoder->ignoreXMP = AVIF_TRUE;
    decoder->imageContentToDecode = AVIF_IMAGE_CONTENT_COLOR_AND_ALPHA | AVIF_IMAGE_CONTENT_GAIN_MAP;
    if (avifDecoderSetIOMemory(decoder, data, size) == AVIF_RESULT_OK && avifDecoderParse(decoder) == AVIF_RESULT_OK) {
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
        describe(data, size, text, sizeof text);
        free(data);
        snprintf(path, sizeof path, "%s/%s.txt", out, names[i]);
        if (!write_text(path, text)) return 1;
        free(names[i]);
    }
    free(names);
    return 0;
}
