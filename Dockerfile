FROM rust:bookworm as builder
RUN apt update && apt install -y ca-certificates pkg-config libssl-dev libclang-11-dev
WORKDIR /src
COPY . /src

RUN mkdir /out
RUN cargo build --release --manifest-path ./Cargo.toml
RUN cp target/release/stacks-devnet-api /out

FROM gcr.io/distroless/cc
COPY --from=builder /out/ /bin/

CMD ["stacks-devnet-api"]