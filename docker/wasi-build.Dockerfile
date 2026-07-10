# Build a Mako .mko to wasm32-wasi using wasi-sdk (Vision Later Partial).
# Usage:
#   docker build -f docker/wasi-build.Dockerfile -t mako-wasi .
#   docker run --rm -v "$PWD/out:/out" mako-wasi
# Produces /out/hello.wasm when the build succeeds.
FROM ghcr.io/webassembly/wasi-sdk:wasi-sdk-22 AS sdk
FROM rust:1-bookworm AS build
COPY --from=sdk /opt/wasi-sdk /opt/wasi-sdk
ENV WASI_SDK_PATH=/opt/wasi-sdk
ENV PATH="${WASI_SDK_PATH}/bin:${PATH}"
WORKDIR /src
COPY . .
RUN cargo build --release \
 && mkdir -p /out \
 && ./target/release/mako build examples/wasi_hello.mko --target wasm32-wasi -o /out/hello.wasm \
 && test -s /out/hello.wasm \
 && echo "wasi build ok: $(wc -c < /out/hello.wasm) bytes"
CMD ["ls", "-la", "/out/hello.wasm"]
