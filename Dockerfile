# Multi-arch image. Pulls the pre-built binary from the corresponding
# GitHub Release (built by cargo-dist) — avoids re-compiling inside Docker.
#
# Runtime base is `gcr.io/distroless/cc-debian12` (~25 MB) because
# rust-analyzer (the `ra_ap_*` crates that power the semantic stage)
# is officially supported only against glibc. That rules out
# musl-based images (alpine, scratch). Distroless cc gives us glibc +
# libstdc++ + ca-certs with no shell or package manager — small surface,
# easy to scan.
#
# Build:
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     --build-arg VERSION=0.1.0-alpha.8 -t fedoroff/code-split .

FROM debian:bookworm-slim AS downloader

ARG VERSION
ARG TARGETARCH

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl xz-utils ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN case "$TARGETARCH" in \
      amd64) RUST_ARCH=x86_64-unknown-linux-gnu ;; \
      arm64) RUST_ARCH=aarch64-unknown-linux-gnu ;; \
      *) echo "unsupported TARGETARCH=$TARGETARCH" && exit 1 ;; \
    esac && \
    curl -fsSL "https://github.com/ffedoroff/code-split/releases/download/v${VERSION}/code-split-${RUST_ARCH}.tar.xz" | tar -xJC /tmp && \
    install -m 0755 "/tmp/code-split-${RUST_ARCH}/code-split" /usr/local/bin/code-split

FROM gcr.io/distroless/cc-debian12

LABEL org.opencontainers.image.source="https://github.com/ffedoroff/code-split"
LABEL org.opencontainers.image.description="Polyglot structural-analysis platform"
LABEL org.opencontainers.image.licenses="Apache-2.0"

COPY --from=downloader /usr/local/bin/code-split /usr/local/bin/code-split

ENTRYPOINT ["/usr/local/bin/code-split"]
