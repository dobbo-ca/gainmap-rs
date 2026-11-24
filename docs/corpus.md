# Parity corpus

The parity run (docs/parity.md) reads public files, and files generated in CI from them with open-source tools. Every file has a clear licence, which
is recorded here and in `tests/parity/corpus.tsv`. No file is kept in this repository:
`tests/parity/fetch.sh` downloads them into the CI cache mount `/corpus`, checks the size (and the
SHA-256 where it is pinned) and prints the SHA-256 of each file, so a "-" in `corpus.tsv` can be
pinned after the first run. No personal photos of ours are used.

Sources, 56 fetched files in all (and 21 generated ones, see "Generated files"):

| Source | Files | Licence | Pinned to |
|---|---|---|---|
| libavif `tests/data` | 30 | BSD-2-Clause (same as libavif) | tag v1.4.2 |
| Mishaal Rahman, Ultra HDR Samples, `Originals` | 10 | CC-BY-4.0 | commit de109721ca47 |
| Wikimedia Commons, category HDR gain-mapped images | 14 | CC BY-SA 4.0 | the file name (Commons keeps old revisions) |
| johncf/apple-hdr-heic | 1 | MIT | commit e64716c29abc, SHA-256 pinned |
| libultrahdr `tests/data` | 1 | Apache-2.0 OR MIT | commit 66821e0a261a |

Where a file came from a person's own photos (some libavif test files say "personal photo"), the
licence is the one the owner of the repository gave it; the libavif `tests/data/README.md` says
"same as libavif" (BSD-2-Clause) for every libavif gain-map file below except
`seine_sdr_gainmap_big_srgb.avif`, which has no per-file line. Its licence is the licence of the
libavif repository it is stored in, BSD-2-Clause (the repository has no other licence for
`tests/data`), and `corpus.tsv` and the table below say so.

## What each group tests

- libavif JPEGs: `seine_sdr_gainmap_srgb.jpg` is a Photoshop file (MPF, XMP with `rdf:Seq`, no
  ISO payload). `paris_exif_xmp_gainmap_*` have little- and big-endian MPF. `apple_gainmap_*` are
  Apple `HDRGainMap` XMP files.
- libavif AVIFs: a `tmap` item with an `altr` group, and the negative cases: wrong `altr` order,
  no `tmap` brand, an unsupported version, an unsupported minimum version, a writer version with
  extra bytes, gamma zero, grids of different shape, a gain map of other size.
- Mishaal Rahman: ten Ultra HDR JPEGs from a Pixel (XMP attributes, Google container, MPF).
- Commons: Pixel 9 files (`HeySong`, XMP attributes and Google container, no ISO payload), a
  Lightroom file (`Echo Park`: XMP `rdf:Seq` and ISO payloads, 21.8 MB) and nine more files that
  the category lists, each under 10 MB. The camera of the nine is not recorded on the file pages;
  all are phone- or camera-sized JPEGs.
- johncf: an iPhone HEIC with an auxiliary `hdrgainmap` image, which has no `tmap` item.
- libultrahdr `gainmap_hevc_16x16.heic` (`uhdr_gainmap_hevc_16x16.heic`): the only licensed HEIC with a `tmap` item that was found. Brands `heic mif1 tmap miaf`, an `altr` group [`tmap`, base], base and gain map both HEVC Main Still Picture 8-bit 4:2:0 coded at 64x64 with an essential `clap` that crops to 16x16, a 3-channel gain map, base with an ICC profile. Synthetic uniform pixels. Not in any libultrahdr tag (after v2.0.2).
- libavif Adobe-aux AVIFs (`colors_hdr_p3`, `colors_hdr_rec2020`, `colors_hdr_srgb`, `colors_text_hdr_p3`, `colors_text_hdr_rec2020`, `colors_text_hdr_srgb`, `colors_text_wcg_hdr_rec2020`, `seine_hdr_srgb`): no `tmap` item; the gain map is an `auxl` image of type `urn:com:photo:aux:hdrgainmap` with `hdrgm` XMP, 10-bit 4:4:4. libavif ignores that format (its README), so these have no libavif gain-map channel; they exist to check that `gainmap` does not drop the gain map (src/heif.rs). `seine_hdr_rec2020.avif` and `colors_wcg_hdr_rec2020.avif` have no aux gain map and are left out.

## Files

Name is the local file name (`corpus.tsv`). Sizes are bytes.

| Name | Size | Licence | Source |
|---|---:|---|---|
| apple_gainmap_new.jpg | 50824 | BSD-2-Clause | libavif v1.4.2 tests/data/apple_gainmap_new.jpg |
| apple_gainmap_old.jpg | 50742 | BSD-2-Clause | libavif v1.4.2 tests/data/apple_gainmap_old.jpg |
| color_grid_alpha_grid_gainmap_nogrid.avif | 2870 | BSD-2-Clause | libavif v1.4.2 tests/data/color_grid_alpha_grid_gainmap_nogrid.avif |
| color_grid_gainmap_different_grid.avif | 3101 | BSD-2-Clause | libavif v1.4.2 tests/data/color_grid_gainmap_different_grid.avif |
| color_nogrid_alpha_nogrid_gainmap_grid.avif | 1875 | BSD-2-Clause | libavif v1.4.2 tests/data/color_nogrid_alpha_nogrid_gainmap_grid.avif |
| paris_exif_xmp_gainmap_bigendian.jpg | 47579 | BSD-2-Clause | libavif v1.4.2 tests/data/paris_exif_xmp_gainmap_bigendian.jpg |
| paris_exif_xmp_gainmap_littleendian.jpg | 47579 | BSD-2-Clause | libavif v1.4.2 tests/data/paris_exif_xmp_gainmap_littleendian.jpg |
| paris_exif_xmp_icc_gainmap_bigendian.jpg | 48117 | BSD-2-Clause | libavif v1.4.2 tests/data/paris_exif_xmp_icc_gainmap_bigendian.jpg |
| seine_hdr_gainmap_small_srgb.avif | 102004 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_hdr_gainmap_small_srgb.avif |
| seine_hdr_gainmap_srgb.avif | 122961 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_hdr_gainmap_srgb.avif |
| seine_hdr_gainmap_wrongaltr.avif | 122961 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_hdr_gainmap_wrongaltr.avif |
| seine_sdr_different_gainmap_srgb.jpg | 168934 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_sdr_different_gainmap_srgb.jpg |
| seine_sdr_gainmap_big_srgb.avif | 169759 | BSD-2-Clause (libavif repo; tests/data/README.md has no per-file line) | libavif v1.4.2 tests/data/seine_sdr_gainmap_big_srgb.avif |
| seine_sdr_gainmap_gammazero.avif | 129766 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_sdr_gainmap_gammazero.avif |
| seine_sdr_gainmap_notmapbrand.avif | 129769 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_sdr_gainmap_notmapbrand.avif |
| seine_sdr_gainmap_srgb.avif | 129773 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_sdr_gainmap_srgb.avif |
| seine_sdr_gainmap_srgb.jpg | 142972 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_sdr_gainmap_srgb.jpg |
| seine_sdr_gainmap_srgb_icc.avif | 129863 | BSD-2-Clause | libavif v1.4.2 tests/data/seine_sdr_gainmap_srgb_icc.avif |
| supported_gainmap_writer_version_with_extra_bytes.avif | 1375 | BSD-2-Clause | libavif v1.4.2 tests/data/supported_gainmap_writer_version_with_extra_bytes.avif |
| unsupported_gainmap_minimum_version.avif | 1371 | BSD-2-Clause | libavif v1.4.2 tests/data/unsupported_gainmap_minimum_version.avif |
| unsupported_gainmap_version.avif | 1371 | BSD-2-Clause | libavif v1.4.2 tests/data/unsupported_gainmap_version.avif |
| unsupported_gainmap_writer_version_with_extra_bytes.avif | 1375 | BSD-2-Clause | libavif v1.4.2 tests/data/unsupported_gainmap_writer_version_with_extra_bytes.avif |
| mishaal_originals_01.jpg | 2746718 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_01.jpg |
| mishaal_originals_02.jpg | 2915755 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_02.jpg |
| mishaal_originals_03.jpg | 7110323 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_03.jpg |
| mishaal_originals_04.jpg | 3319655 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_04.jpg |
| mishaal_originals_05.jpg | 2290959 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_05.jpg |
| mishaal_originals_06.jpg | 6238247 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_06.jpg |
| mishaal_originals_07.jpg | 4394143 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_07.jpg |
| mishaal_originals_08.jpg | 7146648 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_08.jpg |
| mishaal_originals_09.jpg | 5885644 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_09.jpg |
| mishaal_originals_10.jpg | 5706655 | CC-BY-4.0 | MishaalRahmanGH/Ultra_HDR_Samples Originals/Ultra_HDR_Samples_Originals_10.jpg |
| commons_bayview_library.jpg | 6445617 | CC-BY-SA-4.0 | Commons File:Bayview_Linda_Brooks-Burton_Branch_Library,_San_Francisco,_California,_US.jpg |
| commons_puffballs.jpg | 1809690 | CC-BY-SA-4.0 | Commons File:Common_puffballs_popping_out_of_the_ground,_Healy,_Alaska,_US.jpg |
| commons_coso_square.jpg | 7075970 | CC-BY-SA-4.0 | Commons File:Coso_Square_in_Bernal_Heights,_San_Francisco,_California,_US.jpg |
| commons_dacia_duster.jpg | 1104568 | CC-BY-SA-4.0 | Commons File:Dacia_Duster_2025_Front_1_(cropped_2).jpg |
| commons_echo_park.jpg | 21787060 | CC-BY-SA-4.0 | Commons File:Echo_Park_at_sunrise_in_Dinosaur_National_Monument,_Colorado,_US.jpg |
| commons_fiat_panda.jpg | 2851472 | CC-BY-SA-4.0 | Commons File:Fiat_Grande_Panda_Hybrid_2025_Rear_1_(cropped).jpg |
| commons_heysong_1.jpg | 2064177 | CC-BY-SA-4.0 | Commons File:HeySong_Sarsaparilla_and_noodles_at_886_restaurant_1.jpg |
| commons_heysong_2.jpg | 1868200 | CC-BY-SA-4.0 | Commons File:HeySong_Sarsaparilla_and_noodles_at_886_restaurant_2.jpg |
| commons_heysong_3.jpg | 2537271 | CC-BY-SA-4.0 | Commons File:HeySong_Sarsaparilla_and_noodles_at_886_restaurant_3.jpg |
| commons_heysong_4.jpg | 1958573 | CC-BY-SA-4.0 | Commons File:HeySong_Sarsaparilla_and_noodles_at_886_restaurant_4.jpg |
| commons_hotel_adagio.jpg | 6083245 | CC-BY-SA-4.0 | Commons File:Hotel_Adagio,_San_Francisco,_California,_US.jpg |
| commons_saint_marys_park.jpg | 6361642 | CC-BY-SA-4.0 | Commons File:Saint_Mary's_Park,_San_Francisco,_California,_US.jpg |
| commons_shipwrights_cottage.jpg | 7120769 | CC-BY-SA-4.0 | Commons File:Shipwright's_Cottage_in_India_Basin,_San_Francisco,_California,_US.jpg |
| commons_stony_hill.jpg | 2355455 | CC-BY-SA-4.0 | Commons File:Stony_Hill_seen_from_Bernal_Heights,_San_Francisco,_California,_US.jpg |
| apple_hdr_sample.heic | 238982 | MIT | johncf/apple-hdr-heic tests/data/hdr-sample.heic (Git LFS) |
| uhdr_gainmap_hevc_16x16.heic | 1523 (SHA-256 in corpus.tsv, equal to the one in the libultrahdr fixture README) | Apache-2.0 OR MIT (libultrahdr tests/data/GAINMAP_FIXTURE_README.md, "Fixture-specific license") | libultrahdr commit 66821e0 tests/data/gainmap_hevc_16x16.heic |
| colors_hdr_p3.avif, colors_hdr_rec2020.avif, colors_hdr_srgb.avif, colors_text_hdr_p3.avif, colors_text_hdr_rec2020.avif, colors_text_hdr_srgb.avif, colors_text_wcg_hdr_rec2020.avif, seine_hdr_srgb.avif | 26532, 24065, 28030, 29054, 28573, 34458, 33242, 168028 (SHA-256 in corpus.tsv) | BSD-2-Clause (tests/data/README.md: "same as libavif") | libavif v1.4.2 tests/data/<name> |

## Generated files

The public corpus has no HEIF/AVIF gain map with `irot`, `imir` or `clap`, none with a 4:0:0 gain
map, a 10- or 12-bit gain map or a grid gain map, and only one licensed HEIC `tmap`. So CI builds
21 more files, hand-made from corpus inputs by CI-only tools. They are never committed and never
fetched: `tests/parity/gen.sh <corpus>` writes them to `<corpus>/files/gen_*` on every parity run
(names in `<corpus>/gen.names`, which `inputs.sh` reads), and they go through the same channels as
the fetched files (the libavif, libheif and ExifTool channels, the `planes` limit, thread and
cancel checks). Every file is made twice and the outputs are compared; the report lists name,
SHA-256, size and "same bytes" for each. A file listed in `tests/parity/gen-pins.tsv` must have
that SHA-256, or `gen.sh` fails. All 21 are pinned. Sixteen (the 13 AVIFs made from the seine inputs
and 3 of the HEICs) gave the same bytes in runs 45295848457 and 45297466465; the other 5 (the 4
`gen_tmap_ds*` and `gen_uhdr_p010_pq_bt2100.heic`) changed when `UHDR_WRITE_XMP=ON` was added, and
agree between run 45297466465 and run 45299063870. Every file was
also the same on the two runs inside each parity run. A new generator version means new pins.

Tools, each pinned by tag and commit in `.github/ci/build-gen-avif.sh` and `build-gen-heic.sh`,
built in their own Dockerfile stages, CI-only, never a dependency of a crate:

| Tool | Version | Licence | Made |
|---|---|---|---|
| libaom (the AV1 encoder) | v3.14.1 `03087864cf4b` | BSD-2-Clause and the AOM patent licence | |
| libavif `avifenc`, `avifgainmaputil combine` | v1.4.2 `c5240fc79fe5` | BSD-2-Clause | the AVIFs |
| libultrahdr `ultrahdr_app` (`UHDR_WRITE_XMP=ON`) | v2.0.2 `e5f5a022fe96` | MIT OR Apache-2.0 (and the adobe-hdr-gain-map-license NOTICE) | the HEICs and one JPEG |
| libheif `4a3f74bc` with libultrahdr's `libheif_pr1503.patch` (`tmap` writing, open libheif PR 1503) | fetched by libultrahdr's build | LGPL-3.0 | inside `ultrahdr_app` |
| x265 (the HEVC encoder), Debian bookworm 3.5 | | GPL-2.0-or-later | inside `ultrahdr_app` |
| libjpeg-turbo (libultrahdr's own build), libjpeg, libpng, libxml2, zlib (Debian bookworm) | | BSD-3-Clause, IJG, zlib, MIT | the JPEG and PNG readers of the apps |

Settings are fixed: aom speed 6, one thread (`-j 1`), default quality (colour 60, gain map 60), libultrahdr
defaults except where a name says otherwise. Inputs: `seine_sdr_gainmap_srgb.jpg` (base, 400x300
8-bit with a gain map that `combine` ignores) and `seine_hdr_srgb.avif` (the HDR alternate, 10-bit),
both BSD-2-Clause libavif test files from the corpus, and `raw_p010_image.p010`, a 1280x720 P010
image from libultrahdr `tests/data` at commit e5f5a02 (CC-BY-4.0, `tests/data/LICENSE`; it is read
and encoded, never kept). `combine` runs with `--ignore-profile`, because libavif cannot compute a
gain map for an image with an ICC profile.

| File | Made with | What it adds |
|---|---|---|
| `gen_tmap_irot1.avif`, `gen_tmap_irot3_imir1.avif`, `gen_tmap_imir0.avif` | `avifenc --irot/--imir` on the seine JPEG | `irot` and `imir` on the base and the gain-map item of a `tmap` file |
| `gen_tmap_ds.avif`, `gen_tmap_ds_clap.avif`, `gen_tmap_ds_irot2_clap.avif`, `gen_tmap_ds_irot1_imir0_clap.avif` | `ultrahdr_app -s 2` writes a 1280x720 Ultra HDR JPEG with a 640x360 gain map, `avifenc` converts it, with `--crop 8,8,1264,704` (`clap`), `--irot`, `--imir` | `clap`, alone and with `irot` and `imir`, on a `tmap` file with a half-size gain map. libavif writes a `clap` on a `tmap` file only when the gain map is not the size of the base image (`write.c`, `AVIF_RESULT_NOT_IMPLEMENTED` otherwise), so the same-size JPEG cannot carry one |
| `gen_gm_yuv400.avif`, `gen_gm_depth10.avif`, `gen_gm_depth12_yuv400.avif` | `avifgainmaputil combine --yuv-gain-map 400`, `--depth-gain-map 10`, `12` | a 4:0:0 gain map, 10- and 12-bit gain maps |
| `gen_gm_down2.avif`, `gen_gm_down4_yuv420.avif` | `combine --downscaling 2` and `4` (with `--yuv-gain-map 420`) | a gain map of half and a quarter the size |
| `gen_grid2x2.avif`, `gen_grid2x2_gm_yuv400_depth10.avif` | `combine --grid 2x2` | a grid base and a grid gain map (libavif splits both), also with a 4:0:0 10-bit gain map |
| `gen_base10_yuv420.avif`, `gen_base12_yuv422.avif` | `combine --depth 10 --yuv 420`, `--depth 12 --yuv 422` | a 10-bit 4:2:0 and a 12-bit 4:2:2 base |
| `gen_base_hdr10.avif` | `combine <hdr avif> <sdr jpg>` | a 10-bit HDR base (the SDR image is the alternate) |
| `gen_uhdr_p010.heic`, `gen_uhdr_p010_mono.heic` (`-M 0`), `gen_uhdr_p010_down2.heic` (`-s 2`), `gen_uhdr_p010_pq_bt2100.heic` (`-t 2 -C 2`) | `ultrahdr_app -m 0 -a 0` on the P010 image | HEIC `tmap` files with real content: a 3-channel and a single-channel gain map, a half-size gain map, PQ and BT.2100 input |

Still not covered, because no tool in the CI image writes it: a HEIC with `irot`, `imir`, a grid
base or a 10-bit base (`ultrahdr_app` has no option for them, and its libheif patch encodes one
8-bit image per item; a C driver against the patched libheif would be the way, not built), a HEIC
grid gain map (libheif PR 1503 takes one image), and an iOS 18 HEIC (Apple `auxl` gain map plus a
`tmap`; no licensed sample). `parity-arm64` and `bench` do not run `gen.sh` (their images have no generators)
and `inputs.sh` ignores `gen.names` for them, so their inputs depend on the commit and not on the shared cache.

## Attribution

- Mishaal Rahman, Ultra HDR Samples (https://github.com/MishaalRahmanGH/Ultra_HDR_Samples),
  CC BY 4.0. The files are unchanged.
- Commons files, CC BY-SA 4.0, unchanged: HeySong 1 to 4 by Qzekrom; Echo Park, Bayview Library,
  Hotel Adagio, Saint Mary's Park, Coso Square, Stony Hill, Shipwright's Cottage and the
  puffballs by Clyde Charles Brown; the Dacia Duster and the Fiat Grande Panda by Mihnea Lazar.
  The file pages on commons.wikimedia.org hold the full credit and licence text.
- libavif test data: Copyright the libavif authors, BSD-2-Clause.
- johncf/apple-hdr-heic: MIT.
- libultrahdr test fixture `gainmap_hevc_16x16.heic`: Apache-2.0 OR MIT (the libultrahdr authors); unchanged.
- libultrahdr `tests/data/raw_p010_image.p010` (commit e5f5a02), CC BY 4.0, the libultrahdr authors: encoded into the `gen_uhdr_*` and `gen_tmap_ds*` files, which are made in CI and not kept.

## Excluded

- The libultrahdr benchmark zip (206 MB, no licence stated).
- Eric Chan's ISO JPEG set ("All rights reserved").
- Commons files over 10 MB, except `commons_echo_park.jpg` (21.8 MB), which is kept on purpose: it is
  the Lightroom file with XMP `rdf:Seq` and ISO payloads.
- Files that show people, and any file with a licence that is not CC BY, CC BY-SA, BSD or MIT.
- Any file from a private person's library.

## Adding a file

Add a row to `corpus.tsv` (name, url, licence, sha256 or "-", size or "-"), add it here, and run
`fetch.sh`: it prints the SHA-256 to pin. A file that only one side can read gets a line in
`known-differences.tsv` with the reason.
