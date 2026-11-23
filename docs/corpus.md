# Parity corpus

The parity run (docs/parity.md) reads public files only. Every file has a clear licence, which
is recorded here and in `tests/parity/corpus.tsv`. No file is kept in this repository:
`tests/parity/fetch.sh` downloads them into the CI cache mount `/corpus`, checks the size (and the
SHA-256 where it is pinned) and prints the SHA-256 of each file, so a "-" in `corpus.tsv` can be
pinned after the first run. No personal photos of ours are used.

Sources, 56 files in all:

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
| uhdr_gainmap_hevc_16x16.heic | 1523 (SHA-256 in corpus.tsv after the first CI fetch) | Apache-2.0 OR MIT (libultrahdr tests/data/GAINMAP_FIXTURE_README.md, "Fixture-specific license") | libultrahdr commit 66821e0 tests/data/gainmap_hevc_16x16.heic |
| colors_hdr_p3.avif, colors_hdr_rec2020.avif, colors_hdr_srgb.avif, colors_text_hdr_p3.avif, colors_text_hdr_rec2020.avif, colors_text_hdr_srgb.avif, colors_text_wcg_hdr_rec2020.avif, seine_hdr_srgb.avif | 26532, 24065, 28030, 29054, 28573, 34458, 33242, 168028 (SHA-256 in corpus.tsv after the first CI fetch) | BSD-2-Clause (tests/data/README.md: "same as libavif") | libavif v1.4.2 tests/data/<name> |

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
