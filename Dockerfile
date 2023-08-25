FROM rust:bullseye as builder
WORKDIR /src
RUN apt update && apt install -y ca-certificates pkg-config libssl-dev libclang-11-dev
RUN rustup update 1.67.0 && rustup default 1.67.0
COPY . /src

RUN mkdir /out
RUN cargo build --release --manifest-path ./Cargo.toml
RUN cp target/release/stacks-devnet-api /out

FROM debian:bullseye-slim
COPY --from=builder /out/ /bin/

CMD ["stacks-devnet-api"]