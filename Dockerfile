# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM rust:1.99.1-slim-trixie AS builder
ARG TARGETARCH
ARG BUILDARCH
ARG ZIG_VERSION=0.16.0
ARG CARGO_ZIGBUILD_VERSION=0.23.4
ARG ZIG_SHA256_X86_64=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ARG ZIG_SHA256_AARCH64=ea4b09bfb22ec6f6c6ceac57ab63efb6b46e17ab08d21f69f3a48b38e1534f17
ARG CARGO_ZIGBUILD_SHA256_X86_64=6b69963040818ca1ef573d8135a95aa902e17cf7f873c32c6426642dadfe7f59
ARG CARGO_ZIGBUILD_SHA256_AARCH64=25cf2cdc6cde848b33a761ff2cf46d1efe6ba7e1adfc4b6f3ee2d570cacb5419
ARG APP_VERSION=0.0.0-dev

RUN apt-get update \
 && apt-get install -y --no-install-recommends curl xz-utils ca-certificates \
 && rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    case "$BUILDARCH" in \
      amd64) HA=x86_64; ZIG_SHA="$ZIG_SHA256_X86_64"; ZB_SHA="$CARGO_ZIGBUILD_SHA256_X86_64" ;; \
      arm64) HA=aarch64; ZIG_SHA="$ZIG_SHA256_AARCH64"; ZB_SHA="$CARGO_ZIGBUILD_SHA256_AARCH64" ;; \
      *) echo "unsupported build arch $BUILDARCH"; exit 1 ;; \
    esac; \
    curl -fsSL -o /tmp/zig.tar.xz "https://ziglang.org/download/${ZIG_VERSION}/zig-${HA}-linux-${ZIG_VERSION}.tar.xz"; \
    echo "${ZIG_SHA}  /tmp/zig.tar.xz" | sha256sum -c -; \
    tar -xJf /tmp/zig.tar.xz -C /opt; \
    ln -s "/opt/zig-${HA}-linux-${ZIG_VERSION}/zig" /usr/local/bin/zig; \
    curl -fsSL -o /tmp/cargo-zigbuild.tar.xz "https://github.com/rust-cross/cargo-zigbuild/releases/download/v${CARGO_ZIGBUILD_VERSION}/cargo-zigbuild-${HA}-unknown-linux-gnu.tar.xz"; \
    echo "${ZB_SHA}  /tmp/cargo-zigbuild.tar.xz" | sha256sum -c -; \
    tar -xJf /tmp/cargo-zigbuild.tar.xz -C /usr/local/bin --strip-components=1; \
    rm /tmp/zig.tar.xz /tmp/cargo-zigbuild.tar.xz; \
    zig version; \
    cargo-zigbuild --version

RUN set -eux; \
    case "$TARGETARCH" in amd64) T=x86_64-unknown-linux-musl ;; arm64) T=aarch64-unknown-linux-musl ;; *) echo "unsupported target arch $TARGETARCH"; exit 1 ;; esac; \
    echo "$T" > /rust-target; \
    rustup target add "$T"

WORKDIR /src
COPY . .
ENV APP_VERSION=${APP_VERSION}
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=target-${TARGETARCH} \
    set -eux; \
    T="$(cat /rust-target)"; \
    cargo zigbuild --release --locked -p mcp-companies-house --target "$T"; \
    cp "target/$T/release/mcp-companies-house" /mcp-companies-house

FROM scratch
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /mcp-companies-house /mcp-companies-house
USER 65532:65532
EXPOSE 8443
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["/mcp-companies-house", "--healthcheck"]
ENTRYPOINT ["/mcp-companies-house"]
