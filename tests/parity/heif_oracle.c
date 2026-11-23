/* libheif v1.23.7 oracle of gainmap-rs (CI only, docs/parity.md): decodes the base image and the
 * gain-map image of every HEIC file at their native chroma format and bit depth, by item id, and
 * writes the files that `oracle planes` (examples/oracle.rs) writes. Written from the public
 * libheif API (LGPL-3.0 library, linked statically into this CI tool, never shipped, never a
 * dependency of the crate); no libheif code is copied. The decode follows hevc-rs
 * tests/parity/c/heic_oracle.c (ignore_transformations = 1, no colour conversion).
 *
 *   heif_oracle <in-dir> <out-dir> <rust-dir>
 *
 * For every regular file <f> of <in-dir> that has <rust-dir>/<f>.ids (lines "base N" and "gm N":
 * the item ids that gainmap reports), item N is looked up with heif_context_get_image_handle,
 * which finds hidden and auxiliary items too (libheif has no `tmap` support, so the ids come from
 * gainmap). Output per image, <which> = base or gm:
 *   <f>.<which>.yuv   planes Y, Cb, Cr (Y only when monochrome), tight rows, 1 byte per sample at
 *                     depth 8, else 2 bytes little-endian
 *   <f>.<which>.info  "W H chroma depthY depthC" (chroma 400/420/422/444; for a monochrome image
 *                     depthC is depthY), then the transformative properties as "crop L T R B"
 *                     (libheif's borders for the image size W x H), "imir axis" (0 vertical, 1
 *                     horizontal) and "irot degrees", kind by kind in that order and in property
 *                     order within a kind
 * or <f>.<which>.err when libheif cannot decode the image.
 */
#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
#include <errno.h>
#include <libheif/heif.h>
#include <libheif/heif_properties.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#define MAX_FILE ((uint64_t)512 << 20)

/* Reads the whole file; NULL on any failure. *n is the length. */
static uint8_t *read_file(const char *path, size_t *n)
{
    FILE *f = fopen(path, "rb");
    uint8_t *buf;
    long sz;
    if (!f) {
        return NULL;
    }
    if (fseek(f, 0, SEEK_END) != 0 || (sz = ftell(f)) < 0 || fseek(f, 0, SEEK_SET) != 0 ||
        (uint64_t)sz > MAX_FILE) {
        fclose(f);
        return NULL;
    }
    buf = (uint8_t *)malloc((size_t)sz + 1);
    if (!buf || fread(buf, 1, (size_t)sz, f) != (size_t)sz) {
        free(buf);
        fclose(f);
        return NULL;
    }
    fclose(f);
    *n = (size_t)sz;
    return buf;
}

static FILE *open_out(const char *dir, const char *name, const char *which, const char *ext, const char *mode)
{
    char path[4096];
    if (snprintf(path, sizeof path, "%s/%s.%s.%s", dir, name, which, ext) >= (int)sizeof path) {
        return NULL;
    }
    return fopen(path, mode);
}

/* Writes the plane to f at its own depth; returns 0 on success. */
static int put_plane(FILE *f, const heif_image *img, heif_channel ch)
{
    int w = heif_image_get_width(img, ch), h = heif_image_get_height(img, ch);
    int wide = heif_image_get_bits_per_pixel_range(img, ch) > 8;
    size_t stride = 0;
    const uint8_t *p = heif_image_get_plane_readonly2(img, ch, &stride);
    int x, y;
    if (!p || w < 0 || h < 0) {
        return -1;
    }
    for (y = 0; y < h; y++) {
        const uint8_t *row = p + (size_t)y * stride;
        if (!wide) {
            if (fwrite(row, 1, (size_t)w, f) != (size_t)w) {
                return -1;
            }
            continue;
        }
        for (x = 0; x < w; x++) {
            uint16_t v;
            memcpy(&v, row + 2 * (size_t)x, 2);
            if (fputc(v & 0xff, f) == EOF || fputc(v >> 8, f) == EOF) {
                return -1;
            }
        }
    }
    return 0;
}

/* The transformative properties of item `id`, as the lines of the .info file, kind by kind. */
static void put_transforms(FILE *f, const heif_context *ctx, heif_item_id id, int w, int h)
{
    int n = heif_item_get_transformation_properties(ctx, id, NULL, 0);
    heif_property_id *props;
    int kind, i;
    if (n <= 0) {
        return;
    }
    props = (heif_property_id *)malloc((size_t)n * sizeof *props);
    if (!props) {
        return;
    }
    n = heif_item_get_transformation_properties(ctx, id, props, n);
    for (kind = 0; kind < 3; kind++) {
        for (i = 0; i < n; i++) {
            enum heif_item_property_type t = heif_item_get_property_type(ctx, id, props[i]);
            if (kind == 0 && t == heif_item_property_type_transform_crop) {
                int l = 0, tp = 0, r = 0, b = 0;
                heif_item_get_property_transform_crop_borders(ctx, id, props[i], w, h, &l, &tp, &r, &b);
                fprintf(f, "crop %d %d %d %d\n", l, tp, r, b);
            } else if (kind == 1 && t == heif_item_property_type_transform_mirror) {
                fprintf(f, "imir %d\n", (int)heif_item_get_property_transform_mirror(ctx, id, props[i]));
            } else if (kind == 2 && t == heif_item_property_type_transform_rotation) {
                fprintf(f, "irot %d\n", heif_item_get_property_transform_rotation_ccw(ctx, id, props[i]));
            }
        }
    }
    free(props);
}

/* Decodes item `id` into <out>/<name>.<which>.*; returns 1 when an output file could not be
   written. */
static int decode_item(heif_context *ctx, heif_item_id id, const char *name, const char *which, const char *out)
{
    heif_image_handle *hnd = NULL;
    heif_image *img = NULL;
    heif_decoding_options *opt = heif_decoding_options_alloc();
    int bad = 0;
    FILE *f;

    if (!opt || heif_context_get_image_handle(ctx, id, &hnd).code != heif_error_Ok) {
        goto fail;
    }
    opt->ignore_transformations = 1;
    /* Keep the samples as decoded (see hevc-rs heic_oracle.c). */
    opt->output_image_nclx_profile_passthrough = 1;
    if (heif_decode_image(hnd, &img, heif_colorspace_undefined, heif_chroma_undefined, opt).code != heif_error_Ok) {
        goto fail;
    }
    {
        heif_colorspace cs = heif_image_get_colorspace(img);
        heif_chroma ch = heif_image_get_chroma_format(img);
        int mono = cs == heif_colorspace_monochrome && ch == heif_chroma_monochrome;
        int chroma, w = heif_image_get_width(img, heif_channel_Y), h = heif_image_get_height(img, heif_channel_Y);
        if (mono) {
            chroma = 400;
        } else if (cs == heif_colorspace_YCbCr && ch == heif_chroma_420) {
            chroma = 420;
        } else if (cs == heif_colorspace_YCbCr && ch == heif_chroma_422) {
            chroma = 422;
        } else if (cs == heif_colorspace_YCbCr && ch == heif_chroma_444) {
            chroma = 444;
        } else {
            goto fail;
        }
        f = open_out(out, name, which, "yuv", "wb");
        if (!f) {
            bad = 1;
            goto done;
        }
        if (put_plane(f, img, heif_channel_Y) != 0 ||
            (!mono && (put_plane(f, img, heif_channel_Cb) != 0 || put_plane(f, img, heif_channel_Cr) != 0))) {
            bad = 1;
        }
        if (fclose(f) != 0) {
            bad = 1;
        }
        f = open_out(out, name, which, "info", "wb");
        if (!f) {
            bad = 1;
            goto done;
        }
        fprintf(f, "%d %d %d %d %d\n", w, h, chroma, heif_image_get_bits_per_pixel_range(img, heif_channel_Y),
                heif_image_get_bits_per_pixel_range(img, mono ? heif_channel_Y : heif_channel_Cb));
        put_transforms(f, ctx, id, w, h);
        if (fclose(f) != 0) {
            bad = 1;
        }
    }
    goto done;
fail:
    f = open_out(out, name, which, "err", "wb");
    if (f) {
        fputs("decode\n", f);
        fclose(f);
    } else {
        bad = 1;
    }
done:
    if (img) {
        heif_image_release(img);
    }
    if (hnd) {
        heif_image_handle_release(hnd);
    }
    if (opt) {
        heif_decoding_options_free(opt);
    }
    return bad;
}

/* Decodes the items that <rust-dir>/<name>.ids lists; returns 1 when an output file could not be
   written. */
static int decode_file(const char *path, const char *name, const char *out, const char *rust)
{
    char ids_path[4096], line[128];
    FILE *ids;
    heif_context *ctx;
    uint8_t *data;
    size_t len = 0;
    int bad = 0;

    if (snprintf(ids_path, sizeof ids_path, "%s/%s.ids", rust, name) >= (int)sizeof ids_path) {
        return 1;
    }
    ids = fopen(ids_path, "r");
    if (!ids) {
        return 0; /* gainmap did not read the file: nothing to compare */
    }
    ctx = heif_context_alloc();
    data = read_file(path, &len);
    if (!ctx || !data ||
        heif_context_set_security_limits(ctx, heif_get_disabled_security_limits()).code != heif_error_Ok ||
        heif_context_read_from_memory_without_copy(ctx, data, len, NULL).code != heif_error_Ok) {
        /* libheif cannot read the file: no output, which the comparison reports. */
        goto done;
    }
    while (fgets(line, sizeof line, ids)) {
        char which[16];
        unsigned id;
        if (sscanf(line, "%15s %u", which, &id) == 2) {
            bad |= decode_item(ctx, (heif_item_id)id, name, which, out);
        }
    }
done:
    fclose(ids);
    if (ctx) {
        heif_context_free(ctx);
    }
    free(data);
    return bad;
}

int main(int argc, char **argv)
{
    DIR *d;
    struct dirent *e;
    int bad = 0;
    if (argc != 4) {
        fprintf(stderr, "usage: heif_oracle <in-dir> <out-dir> <rust-dir>\n");
        return 2;
    }
    if (mkdir(argv[2], 0777) != 0 && errno != EEXIST) {
        fprintf(stderr, "heif_oracle: %s: %s\n", argv[2], strerror(errno));
        return 2;
    }
    d = opendir(argv[1]);
    if (!d) {
        fprintf(stderr, "heif_oracle: %s: %s\n", argv[1], strerror(errno));
        return 2;
    }
    heif_init(NULL);
    if (!heif_have_decoder_for_format(heif_compression_HEVC)) {
        fprintf(stderr, "heif_oracle: libheif has no HEVC decoder\n");
        return 2;
    }
    while ((e = readdir(d)) != NULL) {
        char path[4096];
        struct stat st;
        if (snprintf(path, sizeof path, "%s/%s", argv[1], e->d_name) >= (int)sizeof path) {
            bad = 1;
            continue;
        }
        if (stat(path, &st) != 0 || !S_ISREG(st.st_mode)) {
            continue;
        }
        bad |= decode_file(path, e->d_name, argv[2], argv[3]);
    }
    closedir(d);
    heif_deinit();
    return bad ? 1 : 0;
}
