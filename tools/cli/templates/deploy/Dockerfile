# syntax=docker/dockerfile:1
FROM rust:1.97-bookworm AS build
ENV RUSTUP_TOOLCHAIN=${RUST_VERSION} PCRE2_SYS_STATIC=1 CARGO_TARGET_DIR=/build/target
ARG OPENRUST_APP_LAYOUT
ARG OPENRUST_FRAMEWORK_LAYOUT
COPY . /application-source/
COPY --from=openrust-source /Cargo.toml /framework-source/
COPY --from=openrust-source /vendor/ /framework-source/vendor/
COPY --from=openrust-source /tools/ /framework-source/tools/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target,sharing=locked \
    set -eu; \
    application="/layout/${OPENRUST_APP_LAYOUT:?缺少应用相对路径}"; \
    framework="/layout/${OPENRUST_FRAMEWORK_LAYOUT:?缺少框架相对路径}"; \
    mkdir -p "$application" "$framework"; \
    cp -a /framework-source/. "$framework/"; \
    cp -a /application-source/. "$application/"; \
    cd "$application"; \
    set -- cargo build --release --bin "{{name}}"; \
    if [ -f Cargo.lock ]; then set -- "$@" --locked; fi; \
    "$@"; \
    install -D -m 755 "$CARGO_TARGET_DIR/release/{{name}}" "/out/bin/{{name}}"; \
    install -m 755 artisan /out/artisan; \
    cp -a public resources /out/; \
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

FROM scratch AS artifacts
COPY --from=build /out/ /

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -o 10001 -g 10001 /app/storage /app/bootstrap/cache
WORKDIR /app
COPY --from=artifacts --chown=10001:10001 / /app/
ENV APP_SERVER_HOST=0.0.0.0 SERVER_PORT=3000
USER 10001:10001
EXPOSE 3000
STOPSIGNAL SIGTERM
ENTRYPOINT ["/app/bin/{{name}}"]
