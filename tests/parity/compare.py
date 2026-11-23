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


def main():
    et_path, oracle_path, known_path, report = sys.argv[1:5]
    with open(et_path, "rb") as f:
        text = f.read().decode("utf-8", "replace")
    et = {os.path.basename(o["SourceFile"]): exiftool_keys(o) for o in json.loads(text, parse_float=str, parse_int=str)} if text.strip() else {}
    oracle = []
    with open(oracle_path, encoding="utf-8") as f:
        for line in f:
            if line.strip():
                oracle.append(json.loads(line))
    known = load_known(known_path)

    stats = Counter()
    mismatches, used = [], set()

    def check(name, where, keys, ours, theirs):
        for key in keys:
            stats["compared"] += 1
            a, b = values(ours.get(key)), values(theirs.get(key))
            if same(a, b):
                stats["match"] += 1
            elif (name, "xmp") in known:
                stats["known"] += 1
                used.add((name, "xmp"))
            else:
                stats["mismatch"] += 1
                mismatches.append((name, where, key, json.dumps(theirs.get(key)), json.dumps(ours.get(key))))

    for o in oracle:
        name = o["SourceFile"]
        if "primary" not in o:
            stats["skipped (no gain map read)"] += 1
            continue
        stats["files"] += 1
        check(name, "primary", PRIMARY_KEYS, o["primary"], et.get(name, {}))
        gm_name = name + ".gm.jpg"
        if gm_name in et:
            check(name, "gainmap", GAINMAP_KEYS, o["gainmap"], et[gm_name])
        else:
            stats["no gain-map jpg for ExifTool"] += 1

    lines = [
        "## XMP parity (ExifTool 13.64)",
        "",
        f"Files with a gain map read by gainmap: {stats['files']}",
        f"Skipped (no gain map read, or no gain-map JPEG for ExifTool): {stats['skipped (no gain map read)']}, {stats['no gain-map jpg for ExifTool']}",
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
