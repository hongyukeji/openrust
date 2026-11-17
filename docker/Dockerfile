FROM --platform=$BUILDPLATFORM rust:1.94.0-bookworm AS build
ARG BUILDARCH
ARG TARGETARCH
ARG OPENRUST_FEATURES=""
ARG OPENRUST_BUILD_JOBS=4
ENV CARGO_TARGET_DIR=/cargo-target
WORKDIR /src

RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-gnu ;; \
      arm64) triple=aarch64-unknown-linux-gnu ;; \
      *) echo "Unsupported Linux architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    if [ "$BUILDARCH" != "$TARGETARCH" ]; then \
      sed -i 's|http://deb.debian.org/|https://deb.debian.org/|g' /etc/apt/sources.list.d/debian.sources; \
      apt-get update; \
      case "$TARGETARCH" in \
        amd64) apt-get install -y --no-install-recommends gcc-x86-64-linux-gnu libc6-dev-amd64-cross ;; \
        arm64) apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross ;; \
      esac; \
      rm -rf /var/lib/apt/lists/*; \
      rustup target add "$triple"; \
    fi; \
    printf '%s\n' "$triple" > /rust-target

COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/cargo-target,id=openrust-target-${TARGETARCH},sharing=locked \
    set -eu; \
    triple="$(cat /rust-target)"; \
    if [ "$BUILDARCH" != "$TARGETARCH" ]; then \
      case "$TARGETARCH" in \
        amd64) export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc ;; \
        arm64) export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc ;; \
      esac; \
    fi; \
    cargo build --locked --release --jobs "$OPENRUST_BUILD_JOBS" --bin main --target "$triple" \
      --features "standalone $OPENRUST_FEATURES"; \
    install -D -m 0755 "$CARGO_TARGET_DIR/$triple/release/main" /out/main

FROM debian:bookworm-slim AS runtime
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN sed -i 's|http://deb.debian.org/|https://deb.debian.org/|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -m 0755 -o 10001 -g 10001 /app/storage

WORKDIR /app
COPY --from=build /out/main /app/main
COPY resources/ /app/resources/
COPY public/ /app/public/
ENV APP_HOST=0.0.0.0 APP_PORT=8000 APP_ENV=production APP_DEBUG=false LOG_CHANNEL=stderr
USER 10001:10001
EXPOSE 8000
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s CMD curl -fsS "http://127.0.0.1:$APP_PORT/up" || exit 1
ENTRYPOINT ["/app/main"]
