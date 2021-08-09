ARG BASE_IMAGE=ekidd/rust-musl-builder:latest

# Our first FROM statement declares the build environment.
FROM ${BASE_IMAGE} AS build

# only build deps for faster builds
COPY Cargo.toml Cargo.toml
USER root
RUN mkdir src/
RUN echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs
RUN cargo build --release
RUN rm -f target/x86_64-unknown-linux-musl/release/deps/pektin*
# Add our source code.
ADD --chown=rust:rust . ./

# Build our application.
RUN cargo build --release
RUN strip target/x86_64-unknown-linux-musl/release/pektin

# Now, we need to build our _real_ Docker container, copying in `using-diesel`.
FROM alpine:latest
WORKDIR /app
#RUN apk --no-cache add ca-certificates
COPY --from=build /home/rust/src/target/x86_64-unknown-linux-musl/release/pektin .

# permissions
RUN addgroup -g 1000 pektin
RUN adduser -D -s /bin/sh -u 1000 -G pektin pektin
RUN chown pektin:pektin pektin
USER pektin

CMD ./pektin