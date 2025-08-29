FROM rust:1.82-bullseye AS build
WORKDIR /app
COPY . .
RUN cargo build --release --locked
RUN sha256sum target/release/the_block > /app/build.sha256

FROM debian:bullseye-slim
COPY --from=build /app/target/release/the_block /usr/local/bin/the_block
COPY --from=build /app/build.sha256 /usr/local/bin/build.sha256
CMD ["/usr/local/bin/the_block"]
