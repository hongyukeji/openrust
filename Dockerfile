# syntax=docker/dockerfile:1
FROM rust:1.97-bookworm AS build
ENV RUSTUP_TOOLCHAIN=${RUST_VERSION} PCRE2_SYS_STATIC=1
ARG APP_BINARY=app
ARG APP_FEATURES=
WORKDIR /workspace
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/workspace/target \
    set -eu; \
    cargo build --release --locked --bin "$APP_BINARY" --features "$APP_FEATURES"; \
    install -D -m 755 "target/release/$APP_BINARY" /out/bin/openrust; \
    install -m 755 artisan /out/artisan; \
    cp -a .env.example public resources /out/; \
    if [ -e lang ] || [ -L lang ]; then cp -a lang /out/; fi; \
    mkdir -p /out/modules; \
    for module in modules/*; do \
        [ -d "$module" ] || continue; \
        destination="/out/$module"; \
        mkdir -p "$destination"; \
        for asset in module.json public resources lang; do \
            if [ -e "$module/$asset" ]; then cp -a "$module/$asset" "$destination/"; fi; \
        done; \
    done

# 用 buildx --target artifacts 导出可交给 Linux 主机的应用文件。
FROM scratch AS artifacts
COPY --from=build /out/ /

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -o 10001 -g 10001 /app/storage /app/bootstrap/cache
WORKDIR /app
COPY --from=artifacts / /app/
ENV APP_SERVER_HOST=0.0.0.0 SERVER_PORT=3000
USER 10001:10001
EXPOSE 3000
STOPSIGNAL SIGTERM
ENTRYPOINT ["/app/bin/openrust"]
