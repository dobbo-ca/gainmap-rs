#!/usr/bin/env python3
"""Compares the XMP values gainmap reads with ExifTool 13.64 (docs/parity.md).

usage: compare.py <exiftool.json> <oracle.jsonl> <known-differences.tsv> <report-dir>

<exiftool.json> is `exiftool -j -n -G1 -a -b -XMP-hdrgm:all -XMP-HDRGainMap:all` over the input
files (SourceFile basename <name>: the primary image) and over the gain-map JPEGs that
`oracle uhdr` wrote (<name>.gm.jpg: the gain-map image). <oracle.jsonl> is `oracle xmp`: per
input the strings gainmap read from the primary XMP ("primary") and from the gain-map XMP
("gainmap"). Only number formatting is normalised: two numbers are equal when their decimal
values are equal (1.0 and 1), a list of one value equals the value, true and false ignore case.
Everything else must be the same text.

Gates (never-drop rule): the file counts as having a gain map when ExifTool shows an hdrgm or
HDRGainMap tag, an MPF image count of 2 or more (MPF:NumberOfImages) or a Google container item
with the semantic GainMap, and every HEIC/HEIF input counts too. If gainmap reports no gain map
for such a file, that is a mismatch, whatever known-differences.tsv says. An input or a gain-map
JPEG that ExifTool has no entry for is a mismatch too (an ExifTool failure must not remove a
check). For a HEIC the Apple values are compared with the ExifTool entry of the file itself
(when ExifTool shows any), and the size of the base image, and of the gain map when ExifTool
lists more than one size, must be one ExifTool reports (ImageWidth, ImageHeight).

Writes <report-dir>/xmp.md and <report-dir>/xmp-mismatches.tsv; the channel name in
known-differences.tsv is `xmp`. Exits 1 on a mismatch that known-differences.tsv does not list.
"""

import json
import os
import re
import sys
from collections import Counter
from decimal import Decimal, InvalidOperation

PRIMARY_KEYS = ["Version"]
GAINMAP_KEYS = [
    "Version", "GainMapMin", "GainMapMax", "Gamma", "OffsetSDR", "OffsetHDR",
    "HDRCapacityMin", "HDRCapacityMax", "BaseRenditionIsHDR",
    "HDRGainMapVersion", "HDRGainMapHeadroom",
]
GROUPS = ("XMP-hdrgm:", "XMP-HDRGainMap:")


def values(v):
    """A value as a list of strings; None (absent) stays None."""
    if v is None:
        return None
    if not isinstance(v, list):
        v = [v]
    out = []
    for x in v:
        if isinstance(x, bool):
            out.append("true" if x else "false")
        else:
            out.append(str(x))
    return out


def norm(s):
    """The canonical form of one value: a Decimal for a number, else the lowercase text for
    true/false, else the text as it is."""
    t = s.strip()
    if t.lower() in ("true", "false"):
        return ("bool", t.lower())
    try:
        return ("num", Decimal(t))
    except InvalidOperation:
        return ("str", s)


def same(a, b):
    if a is None or b is None:
        return a is None and b is None
    return len(a) == len(b) and all(norm(x) == norm(y) for x, y in zip(a, b))


def load_known(path):
    known = {}
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            for line in f:
                parts = line.rstrip("\n").split("\t")
                if line.startswith("#") or len(parts) < 2:
                    continue
                known[(parts[0], parts[1])] = parts[2] if len(parts) > 2 else ""
    return known


def exiftool_keys(obj):
    """{name without group and without ' (n)' suffix: list of strings} of the XMP keys."""
    out = {}
    for key, v in obj.items():
        for g in GROUPS:
            if key.startswith(g):
                name = re.sub(r" \(\d+\)$", "", key[len(g):])
                out.setdefault(name, []).extend(values(v) or [])
    return out


HEIF_EXT = (".heic", ".heif")
JPEG_EXT = (".jpg", ".jpeg")


def sees_gain_map(obj):
    """Why ExifTool thinks the file holds a gain map (a short text), or None. The signs: an
    hdrgm or HDRGainMap XMP tag, an MPF image count of 2 or more, a Google container item whose
    semantic is GainMap."""
    for key, v in obj.items():
        vals = values(v) or []
        if key.startswith(GROUPS):
            return key
        if key == "MPF:NumberOfImages" and any(x.isdigit() and int(x) >= 2 for x in vals):
            return "MPF shows a second image"
        if key.startswith("XMP-GContainer:") and "ItemSemantic" in key and "GainMap" in vals:
            return "GContainer has a GainMap item"
    return None


def sizes(obj, tag):
    """The integers ExifTool reports for a tag such as ImageWidth, in any group."""
    out = []
    for key, v in obj.items():
        if key.endswith(":" + tag):
            out += [x for x in (values(v) or []) if x.isdigit()]
    return out


def main():
    et_path, oracle_path, known_path, report = sys.argv[1:5]
    with open(et_path, "rb") as f:
        text = f.read().decode("utf-8", "replace")
    raw = {os.path.basename(o["SourceFile"]): o for o in json.loads(text, parse_float=str, parse_int=str)} if text.strip() else {}
    et = {name: exiftool_keys(o) for name, o in raw.items()}
    oracle = []
    with open(oracle_path, encoding="utf-8") as f:
        for line in f:
            if line.strip():
                oracle.append(json.loads(line))
    known = load_known(known_path)

    stats = Counter()
    mismatches, used = [], set()

    def report_mismatch(name, where, key, theirs, ours):
        """A mismatch, unless known-differences.tsv lists the file for the xmp channel."""
        if (name, "xmp") in known:
            stats["known"] += 1
            used.add((name, "xmp"))
        else:
            stats["mismatch"] += 1
            mismatches.append((name, where, key, theirs, ours))

    def check(name, where, keys, ours, theirs):
        for key in keys:
            stats["compared"] += 1
            a, b = values(ours.get(key)), values(theirs.get(key))
            if same(a, b):
                stats["match"] += 1
            else:
                report_mismatch(name, where, key, json.dumps(theirs.get(key)), json.dumps(ours.get(key)))

    for o in oracle:
        name = o["SourceFile"]
        is_heif = name.lower().endswith(HEIF_EXT)
        is_jpeg = name.lower().endswith(JPEG_EXT)
        if name not in raw:
            # ExifTool gave nothing for this input: its checks must not silently disappear.
            report_mismatch(name, "exiftool", "entry", "none", "input")
            continue
        seen = sees_gain_map(raw[name])
        if "primary" not in o:
            # gainmap reports no gain map. That is a failure when ExifTool sees one, and for
            # every HEIC of the corpus (each carries an Apple gain map).
            why = "HEIC input" if is_heif else seen
            if why:
                # A gain map the file has but gainmap dropped is never a known difference.
                stats["mismatch"] += 1
                stats["gate failed"] += 1
                mismatches.append((name, "gate", "gain-map-present", json.dumps(why), json.dumps(o.get("Error", "no-gain-map"))))
            else:
                stats["skipped (no gain map read)"] += 1
            continue
        stats["files"] += 1
        check(name, "primary", PRIMARY_KEYS, o["primary"], et.get(name, {}))
        gm_name = name + ".gm.jpg"
        if gm_name in et:
            check(name, "gainmap", GAINMAP_KEYS, o["gainmap"], et[gm_name])
        elif is_jpeg:
            # `oracle uhdr` wrote a gain-map JPEG for a JPEG whose gain map was read, so
            # ExifTool must have an entry for it.
            report_mismatch(name, "exiftool", gm_name, "no entry", "gain-map JPEG written")
        else:
            stats["no gain-map jpg for ExifTool"] += 1
        if is_heif:
            # A HEIC has no gain-map JPEG. The Apple values are compared with what ExifTool
            # reports for the file itself, when it reports any; the sizes of the base image (and
            # of the gain map, when ExifTool lists more than one size) must be ones ExifTool
            # reports.
            if et.get(name):
                check(name, "gainmap", GAINMAP_KEYS, o["gainmap"], et[name])
            else:
                stats["HEIC with no gain-map XMP in ExifTool"] += 1
            for tag, idx in (("ImageWidth", 0), ("ImageHeight", 1)):
                theirs = sizes(raw[name], tag)
                if not theirs or "base" not in o:
                    stats["HEIC with no size in ExifTool"] += 1
                    continue
                want = [o["base"][idx]] + ([o["gm"][idx]] if len(theirs) > 1 else [])
                for w in want:
                    stats["compared"] += 1
                    if str(w) in theirs:
                        stats["match"] += 1
                    else:
                        report_mismatch(name, "size", tag, json.dumps(theirs), json.dumps(w))

    lines = [
        "## XMP parity (ExifTool 13.64)",
        "",
        f"Files with a gain map read by gainmap: {stats['files']}",
        f"Skipped (no gain map read, or no gain-map JPEG for ExifTool): {stats['skipped (no gain map read)']}, {stats['no gain-map jpg for ExifTool']}",
        f"Gain map dropped where ExifTool sees one (or a HEIC): {stats['gate failed']}",
        f"HEIC with no gain-map XMP, or no size, in ExifTool output (not compared): {stats['HEIC with no gain-map XMP in ExifTool']}, {stats['HEIC with no size in ExifTool']}",
        f"Values compared: {stats['compared']}, match {stats['match']}, mismatch {stats['mismatch']}, known {stats['known']}",
        f"Known differences listed but unused: {len({k for k in known if k[1] == 'xmp'} - used)}",
    ]
    if mismatches:
        lines += ["", "First mismatches (all in xmp-mismatches.tsv):", ""]
        lines += [f"- {n} {w} {k}: exiftool {e} gainmap {g}" for n, w, k, e, g in mismatches[:30]]
    out = "\n".join(lines) + "\n"
    with open(os.path.join(report, "xmp.md"), "w", encoding="utf-8") as f:
        f.write(out)
    with open(os.path.join(report, "xmp-mismatches.tsv"), "w", encoding="utf-8") as f:
        f.write("file\timage\tkey\texiftool\tgainmap\n")
        for row in mismatches:
            f.write("\t".join(row) + "\n")
    # The run.sh summary counts: "xmp<TAB>compared<TAB>match<TAB>mismatch<TAB>known".
    with open(os.path.join(report, "xmp.counts"), "w", encoding="utf-8") as f:
        f.write(f"xmp\t{stats['compared']}\t{stats['match']}\t{stats['mismatch']}\t{stats['known']}\n")
    print(out)
    return 1 if mismatches else 0


if __name__ == "__main__":
    sys.exit(main())
