FROM rust:1.76 as build
WORKDIR /usr/src/app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=build /usr/src/app/target/release/explorer /usr/local/bin/explorer
EXPOSE 3001
ENTRYPOINT ["explorer"]
