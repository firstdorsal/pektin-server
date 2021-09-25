# 0. BUILD STAGE
FROM ekidd/rust-musl-builder:beta AS build
# only build deps in the first stage for faster builds
COPY Cargo.toml Cargo.toml
USER root
RUN cargo install cargo-build-deps
RUN cargo build-deps --release
RUN rm -f target/x86_64-unknown-linux-musl/release/deps/pektin*
# build
ADD --chown=rust:rust . ./
RUN cargo build --release --bin pektin_server
RUN strip target/x86_64-unknown-linux-musl/release/pektin_server

# 1. APP STAGE
FROM alpine:latest
WORKDIR /app
COPY --from=build /home/rust/src/target/x86_64-unknown-linux-musl/release/pektin_server ./pektin_server
# permissions
RUN addgroup -g 1000 pektin_server
RUN adduser -D -s /bin/sh -u 1000 -G pektin_server pektin_server
RUN chown pektin_server:pektin_server pektin_server
USER pektin_server
# run it 
CMD ./pektin_server