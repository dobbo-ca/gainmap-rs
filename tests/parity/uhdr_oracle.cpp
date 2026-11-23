// CI-only parity oracle: libultrahdr v2.0.2 (probe, metadata, coded base image and gain map)
// and libjpeg-turbo 3.2.0 (TurboJPEG 3 API, accurate DCT, fancy upsampling) for the pixels.
// Never built on a developer machine and never a dependency of the crate (docs/parity.md).
//
//   uhdr_oracle <in-dir> <out-dir> [--no-raw]
//
// For every regular file <f> of <in-dir> (sorted) it writes <out-dir>/<f>.txt and, when
// libultrahdr finds a base image and a gain map, <f>.base.jpg and <f>.gm.jpg (the bytes of
// uhdr_dec_get_base_image / uhdr_dec_get_gainmap_image), and, when tj3 decodes them,
// <f>.base.rgba (RGBA8888) and <f>.gm.raw (8-bit gray, or RGBA8888 for a 3-component map).
// The text is the one examples/oracle.rs writes (docs/parity.md). "status error" stands for any
// libultrahdr or libjpeg-turbo failure. --no-raw skips the raw files (the bench times it).
//
// libultrahdr's decoder does not read MPF: its JpegScanner takes the first two JPEG images of
// the file (jpegr.cpp, extractPrimaryImageAndGainMap). uhdr_dec_get_base_image returns the bytes
// of the first, uhdr_dec_get_gainmap_image the bytes of the second.
//
// Copyright and licence: this file is part of gainmap-rs (Apache-2.0). It calls the public API
// of libultrahdr (Apache-2.0 / MIT) and libjpeg-turbo (BSD-3-Clause, IJG, zlib).

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <string>
#include <vector>

#include <turbojpeg.h>

#include "ultrahdr_api.h"

namespace fs = std::filesystem;

static bool read_file(const fs::path& p, std::vector<uint8_t>* out) {
  FILE* f = fopen(p.string().c_str(), "rb");
  if (!f) return false;
  uint8_t buf[1 << 16];
  size_t n;
  out->clear();
  while ((n = fread(buf, 1, sizeof buf, f)) > 0) out->insert(out->end(), buf, buf + n);
  fclose(f);
  return true;
}

static bool write_file(const fs::path& p, const void* data, size_t size) {
  FILE* f = fopen(p.string().c_str(), "wb");
  if (!f) return false;
  bool ok = size == 0 || fwrite(data, 1, size, f) == size;
  return fclose(f) == 0 && ok;
}

// The 8 hex digits of the bits of a float, as the Rust oracle prints them.
static std::string hex(float v) {
  uint32_t u;
  memcpy(&u, &v, sizeof u);
  char b[16];
  snprintf(b, sizeof b, "%08x", u);
  return b;
}

static std::string hex3(const float v[3]) { return hex(v[0]) + " " + hex(v[1]) + " " + hex(v[2]); }

// Decodes one JPEG with tj3: RGBA8888, or gray when the JPEG has one component. A libjpeg warning
// (truncated data, ...) is an error, as in the strict Rust decode.
static bool decode_jpeg(const uint8_t* data, size_t size, std::vector<uint8_t>* out, int* channels) {
  tjhandle h = tj3Init(TJINIT_DECOMPRESS);
  if (!h) return false;
  bool ok = false;
  do {
    if (tj3Set(h, TJPARAM_STOPONWARNING, 1) < 0) break;
    if (tj3Set(h, TJPARAM_MAXPIXELS, 1 << 30) < 0) break;
    if (tj3DecompressHeader(h, data, size) < 0) break;
    int w = tj3Get(h, TJPARAM_JPEGWIDTH);
    int ht = tj3Get(h, TJPARAM_JPEGHEIGHT);
    int cs = tj3Get(h, TJPARAM_COLORSPACE);
    if (w <= 0 || ht <= 0) break;
    int fmt = cs == TJCS_GRAY ? TJPF_GRAY : TJPF_RGBA;
    *channels = cs == TJCS_GRAY ? 1 : 4;
    out->assign((size_t)w * (size_t)ht * (size_t)*channels, 0);
    // pitch 0: width * bytes per pixel.
    if (tj3Decompress8(h, data, size, out->data(), 0, fmt) < 0) break;
    ok = true;
  } while (0);
  tj3Destroy(h);
  return ok;
}

struct Result {
  std::string text = "status error\n";
  std::vector<std::pair<std::string, std::vector<uint8_t>>> files;
};

static Result process(const std::vector<uint8_t>& data) {
  Result r;
  uhdr_codec_private_t* dec = uhdr_create_decoder();
  if (!dec) return r;
  do {
    uhdr_compressed_image_t img;
    memset(&img, 0, sizeof img);
    img.data = const_cast<uint8_t*>(data.data());
    img.data_sz = data.size();
    img.capacity = data.size();
    img.cg = UHDR_CG_UNSPECIFIED;
    img.ct = UHDR_CT_UNSPECIFIED;
    img.range = UHDR_CR_UNSPECIFIED;
    if (data.empty() || uhdr_dec_set_image(dec, &img).error_code != UHDR_CODEC_OK) break;
    if (uhdr_dec_probe(dec).error_code != UHDR_CODEC_OK) break;

    uhdr_mem_block_t* base = uhdr_dec_get_base_image(dec);
    uhdr_mem_block_t* gm = uhdr_dec_get_gainmap_image(dec);
    uhdr_gainmap_metadata_t* md = uhdr_dec_get_gainmap_metadata(dec);
    if (!base || !gm || !md || !base->data || !gm->data) break;
    const uint8_t* bp = static_cast<const uint8_t*>(base->data);
    const uint8_t* gp = static_cast<const uint8_t*>(gm->data);
    r.files.push_back({"base.jpg", std::vector<uint8_t>(bp, bp + base->data_sz)});
    r.files.push_back({"gm.jpg", std::vector<uint8_t>(gp, gp + gm->data_sz)});

    std::vector<uint8_t> base_px, gm_px;
    int base_ch = 0, gm_ch = 0;
    if (!decode_jpeg(bp, base->data_sz, &base_px, &base_ch)) break;
    if (!decode_jpeg(gp, gm->data_sz, &gm_px, &gm_ch)) break;
    r.files.push_back({"base.rgba", base_px});
    r.files.push_back({"gm.raw", gm_px});

    std::string t = "status ok\n";
    t += "base.width " + std::to_string(uhdr_dec_get_image_width(dec)) + "\n";
    t += "base.height " + std::to_string(uhdr_dec_get_image_height(dec)) + "\n";
    t += "gainmap.width " + std::to_string(uhdr_dec_get_gainmap_width(dec)) + "\n";
    t += "gainmap.height " + std::to_string(uhdr_dec_get_gainmap_height(dec)) + "\n";
    t += "gainmap.channels " + std::to_string(gm_ch) + "\n";
    t += "max_content_boost " + hex3(md->max_content_boost) + "\n";
    t += "min_content_boost " + hex3(md->min_content_boost) + "\n";
    t += "gamma " + hex3(md->gamma) + "\n";
    t += "offset_sdr " + hex3(md->offset_sdr) + "\n";
    t += "offset_hdr " + hex3(md->offset_hdr) + "\n";
    t += "hdr_capacity_min " + hex(md->hdr_capacity_min) + "\n";
    t += "hdr_capacity_max " + hex(md->hdr_capacity_max) + "\n";
    t += std::string("use_base_cg ") + (md->use_base_cg ? "1" : "0") + "\n";
    r.text = t;
  } while (0);
  uhdr_release_decoder(dec);
  return r;
}

int main(int argc, char** argv) {
  if (argc < 3) {
    fprintf(stderr, "usage: uhdr_oracle <in-dir> <out-dir> [--no-raw]\n");
    return 2;
  }
  bool no_raw = argc > 3 && strcmp(argv[3], "--no-raw") == 0;
  fs::path in = argv[1], out = argv[2];
  std::error_code ec;
  fs::create_directories(out, ec);
  std::vector<std::string> names;
  for (const auto& e : fs::directory_iterator(in, ec)) {
    // status() follows symlinks.
    if (fs::is_regular_file(fs::status(e.path(), ec))) names.push_back(e.path().filename().string());
  }
  std::sort(names.begin(), names.end());
  std::vector<uint8_t> data;
  for (const auto& name : names) {
    if (!read_file(in / name, &data)) {
      fprintf(stderr, "uhdr_oracle: cannot read %s\n", name.c_str());
      return 1;
    }
    Result r = process(data);
    if (!no_raw) {
      for (const auto& f : r.files) {
        if (!write_file(out / (name + "." + f.first), f.second.data(), f.second.size())) return 1;
      }
    }
    if (!write_file(out / (name + ".txt"), r.text.data(), r.text.size())) return 1;
  }
  return 0;
}
