#!/bin/sh
# Fetches the public parity corpus listed in tests/parity/corpus.tsv into <dir>/files:
# tests/parity/fetch.sh <dir>   (CI uses the cache mount /corpus)
#
# corpus.tsv columns (tab-separated): name, url, licence, sha256 or "-", size in bytes or "-".
# <dir>/files/<name>.ok is the stamp of a finished file. A file with the stamp is checked again
# on every run (size and sha256 against corpus.tsv) and kept when it matches; a file without the
# stamp, or that no longer matches, is removed and fetched again. A size or sha256 that is not
# "-" must match, or the script fails. It prints name, sha256 and size of every file it
# fetched, so a "-" can be pinned later (docs/corpus.md).
#
# Only public files with a clear licence are listed (docs/corpus.md). The User-Agent says what
# the request is, with no personal data.
set -eu
dir=${1:?usage: fetch.sh <dir>}
here=$(cd "$(dirname "$0")" && pwd)
ua="gainmap-rs-parity-ci/1.0 (public test-corpus fetch)"
files="$dir/files"
mkdir -p "$files"
tab=$(printf '\t')
fetched=0
failed=0
ready=0

# Rows without comments, as "name<TAB>url<TAB>licence<TAB>sha256<TAB>size".
grep -v '^#' "$here/corpus.tsv" | grep -v '^$' > "$dir/corpus.rows"

while IFS="$tab" read -r name url licence sha size; do
  # A stamped file is checked again against corpus.tsv on every run (size, then sha256: cheap
  # next to a fetch), because the cache is shared between branches and corpus.tsv may have been
  # re-pinned. A file that no longer matches is fetched again.
  if [ -e "$files/$name.ok" ] && [ -f "$files/$name" ]; then
    have_size=$(wc -c < "$files/$name" | tr -d ' ')
    have_sha=$(sha256sum "$files/$name" | cut -d' ' -f1)
    if { [ "$size" = "-" ] || [ "$size" = "$have_size" ]; } && { [ "$sha" = "-" ] || [ "$sha" = "$have_sha" ]; }; then
      ready=$((ready + 1))
      continue
    fi
    echo "fetch.sh: cached $name no longer matches corpus.tsv, fetching it again" >&2
  fi
  rm -f "$files/$name" "$files/$name.ok" "$files/$name.part"
  if ! curl -fsSL --limit-rate 20M --retry 5 --retry-delay 10 --retry-all-errors --connect-timeout 30 \
       -A "$ua" -o "$files/$name.part" "$url"; then
    echo "fetch.sh: cannot fetch $name from $url" >&2
    failed=$((failed + 1))
    rm -f "$files/$name.part"
    continue
  fi
  got_size=$(wc -c < "$files/$name.part" | tr -d ' ')
  got_sha=$(sha256sum "$files/$name.part" | cut -d' ' -f1)
  if [ "$size" != "-" ] && [ "$size" != "$got_size" ]; then
    echo "fetch.sh: $name is $got_size bytes, corpus.tsv says $size" >&2
    failed=$((failed + 1))
    rm -f "$files/$name.part"
    continue
  fi
  if [ "$sha" != "-" ] && [ "$sha" != "$got_sha" ]; then
    echo "fetch.sh: $name has sha256 $got_sha, corpus.tsv says $sha" >&2
    failed=$((failed + 1))
    rm -f "$files/$name.part"
    continue
  fi
  mv "$files/$name.part" "$files/$name"
  touch "$files/$name.ok"
  echo "fetched $name sha256=$got_sha size=$got_size licence=$licence"
  fetched=$((fetched + 1))
  ready=$((ready + 1))
  # Be polite to the hosts: one request at a time with a short pause.
  sleep 1
done < "$dir/corpus.rows"
rm -f "$dir/corpus.rows"

echo "corpus: $ready files ready, $fetched fetched now, $failed failed"
[ "$failed" -eq 0 ]
