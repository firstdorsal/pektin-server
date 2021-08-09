# 0. BUILD STAGE
FROM ekidd/rust-musl-builder:latest AS build
# only build deps in the first stage for faster builds
COPY Cargo.toml Cargo.toml
USER root
RUN mkdir src/
RUN echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs
RUN cargo build --release
RUN rm -f target/x86_64-unknown-linux-musl/release/deps/pektin*
# build
ADD --chown=rust:rust . ./
RUN cargo build --release
RUN strip target/x86_64-unknown-linux-musl/release/pektin

# 1. APP STAGE
FROM alpine:latest
WORKDIR /app
COPY --from=build /home/rust/src/target/x86_64-unknown-linux-musl/release/pektin .
# permissions
RUN addgroup -g 1000 pektin
RUN adduser -D -s /bin/sh -u 1000 -G pektin pektin
RUN chown pektin:pektin pektin
USER pektin
# run it 
CMD ./pektin