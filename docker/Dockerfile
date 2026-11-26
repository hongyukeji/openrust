FROM --platform=$BUILDPLATFORM rust:1.95.0-bookworm AS build
ARG BUILDARCH
ARG TARGETARCH
ARG APP_BUILD_FEATURES=""
ARG APP_BUILD_JOBS=4
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
    features="standalone $APP_BUILD_FEATURES"; \
    cargo build --locked --release --jobs "$APP_BUILD_JOBS" --bin main --target "$triple" --features "$features"; \
    install -D -m 0755 "$CARGO_TARGET_DIR/$triple/release/main" /out/main; \
    install -m 0644 "$(rustc --print sysroot)/share/doc/rust/COPYRIGHT-library.html" /out/COPYRIGHT-library.html; \
    packages="$(cargo tree --locked --offline --target "$triple" --features "$features" \
      --edges normal --prefix none --format '{p}')"; \
    for package in webhook-client webhook-server; do \
      if printf '%s\n' "$packages" | grep -q "^openrust-$package "; then \
        notices="/out/licenses/$package"; \
        install -D -m 0644 "vendor/openrust/$package/THIRD_PARTY_NOTICES.md" "$notices/THIRD_PARTY_NOTICES.md"; \
        install -d -m 0755 "$notices/LICENSES"; \
        cp -R "vendor/openrust/$package/LICENSES/." "$notices/LICENSES/"; \
      fi; \
    done

FROM debian:bookworm-slim AS runtime
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN sed -i 's|http://deb.debian.org/|https://deb.debian.org/|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -m 0755 -o 10001 -g 10001 /app/storage

WORKDIR /app
COPY --from=build /out/ /app/
COPY resources/ /app/resources/
COPY public/ /app/public/
COPY LICENSE THIRD_PARTY_NOTICES.md /app/
ENV APP_HOST=0.0.0.0 APP_PORT=8000 APP_ENV=production APP_DEBUG=false LOG_CHANNEL=stderr
ENV APP_PACKAGES_CACHE=storage/framework/cache/packages.json
USER 10001:10001
EXPOSE 8000
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s CMD curl -fsS "http://127.0.0.1:$APP_PORT/up" || exit 1
ENTRYPOINT ["/app/main"]
