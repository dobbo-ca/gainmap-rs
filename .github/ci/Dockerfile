# CI image: Rust runs here, on the in-cluster BuildKit, because the ARC runner has no toolchain.
# The crate depends on five private dobbo-ca ports by git rev (jpeg-rs, exif-rs, isobmff-rs, hevc-rs,
# av1-rs), and those depend on a sixth (deflate-rs). The fetch is the only step that needs the GitHub App
# token, passed as the BuildKit secret `gh_token` (never a build arg, never written to a layer);
# every later step runs offline. jpeg-rs also pulls rayon from crates.io, and the fuzz crate pulls
# libfuzzer-sys, so the same step fetches the registry crates too.
# Every tool is installed in a stage before `COPY . .`, so a new commit does not install it again,
# and the fetch runs from the manifests and lockfiles alone, so a commit that leaves them alone
# does not fetch again.
FROM rust:1.99.1-bookworm AS toolchain
RUN rustup component add rustfmt clippy && rustup target add aarch64-unknown-linux-gnu
WORKDIR /src

# Only the manifests, with empty targets for what they name, so cargo can resolve them.
FROM toolchain AS deps
COPY Cargo.toml Cargo.lock ./
COPY tests/fuzz/Cargo.toml tests/fuzz/Cargo.lock tests/fuzz/
RUN mkdir -p src tests/fuzz/fuzz_targets && touch src/lib.rs \
    && for t in read decode iso mpf xmp scan; do touch tests/fuzz/fuzz_targets/$t.rs; done
RUN --mount=type=secret,id=gh_token git config --global url."https://x-access-token:$(cat /run/secrets/gh_token)@github.com/".insteadOf "https://github.com/" && CARGO_NET_GIT_FETCH_WITH_CLI=true cargo fetch --locked && CARGO_NET_GIT_FETCH_WITH_CLI=true cargo fetch --locked --manifest-path tests/fuzz/Cargo.toml && git config --global --unset-all url."https://x-access-token:$(cat /run/secrets/gh_token)@github.com/".insteadOf \
    && rm -rf src tests/fuzz/fuzz_targets

FROM toolchain AS src
COPY --from=deps /usr/local/cargo/registry /usr/local/cargo/registry
COPY --from=deps /usr/local/cargo/git /usr/local/cargo/git
ENV CARGO_NET_OFFLINE=true
COPY . .

# The checks run in the build, so a red check fails the build; a rerun of the same source is a
# BuildKit cache hit. The target dirs are cache mounts, so they persist across builds. The target
# cache is shared by every branch and cargo compares mtimes: another branch's newer build would be
# reused as this one's, so every RUN that runs cargo touches the sources just before it starts,
# and holds the cache with sharing=locked, so a concurrent build of another commit cannot build
# into it between the touch and cargo. No SIMD here, so there is no scalar-tier run: this crate has
# no kernel and forbids unsafe code; the SIMD kernels, and their scalar tier, are jpeg-rs's, and
# its own CI runs them both ways.
FROM src AS ci
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cargo fmt --check && cargo fmt --check --manifest-path tests/fuzz/Cargo.toml
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cargo clippy --locked --keep-going --all-targets -- -D warnings
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cargo clippy --locked --keep-going --all-targets --target aarch64-unknown-linux-gnu -- -D warnings
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cargo test --locked
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cargo clippy --locked --keep-going --all-targets --no-default-features -- -D warnings
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cargo test --locked --no-default-features
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cargo build --locked --release --target aarch64-unknown-linux-gnu
# The fuzz crate (libfuzzer-sys) has to compile; fuzz.yml runs its targets.
RUN --mount=type=cache,id=gainmap-fuzz-target,target=/src/tests/fuzz/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cd tests/fuzz && cargo check --locked --bins

# qemu-user and the aarch64 cross toolchain, for the arm64 stages.
FROM toolchain AS qemu-tools
RUN apt-get update && apt-get install -y --no-install-recommends qemu-user gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
    && rm -rf /var/lib/apt/lists/*
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER="qemu-aarch64 -L /usr/aarch64-linux-gnu"

FROM qemu-tools AS qemu-arm64
COPY --from=deps /usr/local/cargo/registry /usr/local/cargo/registry
COPY --from=deps /usr/local/cargo/git /usr/local/cargo/git
ENV CARGO_NET_OFFLINE=true
COPY . .

# The whole suite runs on aarch64 under qemu-user, released: the crate is small and has no SIMD of
# its own, so there are no kernel-only tests to select. What the target changes is jpeg-rs's NEON
# kernels, which every decode of the tests runs.
FROM qemu-arm64 AS arm64
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + && cargo test --locked --release --target aarch64-unknown-linux-gnu

# The parity oracles, built from source, each pinned by tag and commit (docs/parity.md):
# libjpeg-turbo 3.2.0, libultrahdr v2.0.2, libavif v1.4.2 (with dav1d), libde265 v1.0.15, libheif v1.23.7
# and ExifTool 13.64. CI-only,
# installed before `COPY . .`; never a crate dependency. The script is the only file this stage
# copies, so a commit that leaves it alone does not build them again.
FROM toolchain AS oracle-libs
RUN apt-get update && apt-get install -y --no-install-recommends cmake g++ make git ca-certificates curl pkg-config libdav1d-dev \
    && rm -rf /var/lib/apt/lists/*
COPY .github/ci/build-oracle-libs.sh /build-oracle-libs.sh
RUN sh /build-oracle-libs.sh

# The three C tools of tests/parity (uhdr_oracle.cpp, avif_oracle.c, heif_oracle.c) and python3 for compare.py;
# perl is in the base image. Only the two sources are copied, so the tools are rebuilt only when
# they change. libjpeg and libturbojpeg come from /opt/oracle, not from the bookworm libjpeg-dev.
FROM oracle-libs AS oracle-tools
RUN apt-get update && apt-get install -y --no-install-recommends python3 \
    && rm -rf /var/lib/apt/lists/*
COPY tests/parity/uhdr_oracle.cpp tests/parity/avif_oracle.c tests/parity/heif_oracle.c /oracle-src/
RUN mkdir -p /opt/oracle/bin \
    && g++ -O2 -std=c++17 -I/opt/oracle/include /oracle-src/uhdr_oracle.cpp -o /opt/oracle/bin/uhdr_oracle \
       /opt/oracle/lib/libuhdr.a /opt/oracle/lib/libturbojpeg.a /opt/oracle/lib/libjpeg.a -lpthread \
    && gcc -O2 -std=gnu11 -I/opt/oracle/include /oracle-src/avif_oracle.c -o /opt/oracle/bin/avif_oracle \
       /opt/oracle/lib/libavif.a /usr/lib/x86_64-linux-gnu/libdav1d.a -lm -lpthread -ldl \
    && export PKG_CONFIG_PATH=/opt/oracle/lib/pkgconfig \
    && pkg-config --static --libs libheif | grep -q de265 \
    && gcc -O2 -std=gnu99 -Wall -c -o /tmp/heif_oracle.o /oracle-src/heif_oracle.c $(pkg-config --cflags libheif) \
    && g++ -o /opt/oracle/bin/heif_oracle /tmp/heif_oracle.o -static-libstdc++ -static-libgcc $(pkg-config --static --libs libheif)

# Parity: fetch.sh puts the public corpus into the shared /corpus cache mount (once per BuildKit
# cache), run.sh compares examples/oracle.rs with libultrahdr + libjpeg-turbo (pixels and metadata),
# libavif (fractions) and ExifTool (XMP) over it, runs the mutations through the Rust oracle, and
# writes /report (summary.md, status). The step never fails on a parity difference, so the report
# is kept; the workflow fails the job when /report/status is not 0 (if run.sh wrote no status, the
# status of its exit is written here).
FROM oracle-tools AS parity
COPY --from=deps /usr/local/cargo/registry /usr/local/cargo/registry
COPY --from=deps /usr/local/cargo/git /usr/local/cargo/git
ENV CARGO_NET_OFFLINE=true
COPY . .
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    --mount=type=cache,id=gainmap-corpus,target=/corpus,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + \
    && sh tests/parity/fetch.sh /corpus \
    && cargo build --locked --release --example oracle \
    && { sh tests/parity/run.sh /corpus target/release/examples/oracle /report; st=$?; \
         mkdir -p /report; [ -s /report/status ] || echo $st > /report/status; }

# The report as build output: docker/build-push-action with `outputs: type=local,dest=parity-out`.
FROM scratch AS parity-report
COPY --from=parity /report /

# The same oracle example built for x86_64 and for aarch64 (run under qemu-user) reads every 4th
# input; `oracle uhdr`, `avif`, `planes` and `xmp` outputs (text, coded bytes and pixels) must be
# byte-identical. The outputs of one input are deleted before the next, so the layer stays small.
FROM qemu-tools AS parity-arm64
COPY --from=deps /usr/local/cargo/registry /usr/local/cargo/registry
COPY --from=deps /usr/local/cargo/git /usr/local/cargo/git
ENV CARGO_NET_OFFLINE=true
COPY . .
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    --mount=type=cache,id=gainmap-corpus,target=/corpus,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + \
    && sh tests/parity/fetch.sh /corpus \
    && cargo build --locked --release --example oracle \
    && cargo build --locked --release --target aarch64-unknown-linux-gnu --example oracle \
    && sh tests/parity/arm64.sh /corpus target/release/examples/oracle \
       "qemu-aarch64 -L /usr/aarch64-linux-gnu target/aarch64-unknown-linux-gnu/release/examples/oracle" 4

# Fuzzing (fuzz.yml): nightly and cargo-fuzz, with the targets built into the image. The workflow
# runs it with `docker run`, so a BuildKit cache hit cannot skip the fuzzing. No corpus in the
# image: the seeds are made by tests/fuzz/make-seeds.sh (printf, no binary fixtures in git).
FROM toolchain AS fuzz-tools
RUN rustup toolchain install nightly --profile minimal \
    && cargo install cargo-fuzz --locked --version 0.13.1

FROM fuzz-tools AS fuzz
COPY --from=deps /usr/local/cargo/registry /usr/local/cargo/registry
COPY --from=deps /usr/local/cargo/git /usr/local/cargo/git
ENV CARGO_NET_OFFLINE=true
COPY . .
RUN cargo +nightly fuzz build --fuzz-dir tests/fuzz
RUN sh tests/fuzz/make-seeds.sh /seeds && find /seeds -type f | wc -l

# Benchmark (bench.yml): libultrahdr + libjpeg-turbo and the Rust bench example over the same
# files in one image; tests/parity/bench.sh writes /report/bench.md. Information only. The
# workflow builds it with no-cache-filters: bench so the timing runs every time.
FROM oracle-tools AS bench
COPY --from=deps /usr/local/cargo/registry /usr/local/cargo/registry
COPY --from=deps /usr/local/cargo/git /usr/local/cargo/git
ENV CARGO_NET_OFFLINE=true
COPY . .
RUN --mount=type=cache,id=gainmap-target,target=/src/target,sharing=locked \
    --mount=type=cache,id=gainmap-corpus,target=/corpus,sharing=locked \
    find src examples tests -name '*.rs' -exec touch {} + \
    && sh tests/parity/fetch.sh /corpus \
    && sh tests/parity/inputs.sh /corpus /work/in \
    && cargo build --locked --release --example bench \
    && sh tests/parity/bench.sh /work/in /opt/oracle/bin/uhdr_oracle target/release/examples/bench /report \
    && rm -rf /work/in

FROM scratch AS bench-report
COPY --from=bench /report /
