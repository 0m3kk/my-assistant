# Disposable builder for wasm tools.
#
# The container has a Rust toolchain + wasm32-wasip1 target and a vendored
# copy of serde_json, so runtime builds work with `--network none`.
FROM rust:1.85-slim

RUN rustup target add wasm32-wasip1

# Prefetch + vendor serde_json for offline builds.
RUN mkdir -p /opt/prefetch/src \
    && cd /opt/prefetch \
    && printf '[package]\nname = "prefetch"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\nserde_json = "1"\n' > Cargo.toml \
    && echo 'fn main() {}' > src/main.rs \
    && cargo generate-lockfile \
    && cargo vendor /opt/vendor > /dev/null

RUN mkdir -p /usr/local/cargo \
    && printf '[source.crates-io]\nreplace-with = "vendored-sources"\n\n[source.vendored-sources]\ndirectory = "/opt/vendor"\n' > /usr/local/cargo/config.toml

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

WORKDIR /work
ENTRYPOINT ["/entrypoint.sh"]
