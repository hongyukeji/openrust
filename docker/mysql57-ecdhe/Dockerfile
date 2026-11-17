FROM --platform=$BUILDPLATFORM rust:1.94.0-bookworm AS build
ARG TARGETARCH
WORKDIR /src
COPY ecdhe.rs .
RUN test "$TARGETARCH" = amd64 \
    && rustup target add x86_64-unknown-linux-gnu \
    && rustc --edition=2024 --target=x86_64-unknown-linux-gnu --crate-type=cdylib \
        -D warnings -O -C panic=abort -C linker=rust-lld -C linker-flavor=ld.lld \
        -C default-linker-libraries=no -C link-arg=-z -C link-arg=now \
        -o /libmysql57-ecdhe.so ecdhe.rs

FROM mysql:5.7@sha256:4bc6bc963e6d8443453676cae56536f4b8156d78bae03c0145cbe47c2aad73bb
RUN test "$(rpm -q openssl-libs)" = openssl-libs-1.0.2k-26.el7_9.x86_64 \
    && /usr/sbin/mysqld --version | grep -F 'Ver 5.7.44'
COPY --from=build --chmod=0444 /libmysql57-ecdhe.so /usr/local/lib/libmysql57-ecdhe.so
COPY --chmod=0555 mysqld /usr/local/bin/mysqld
