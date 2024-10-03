FROM rust:bullseye as builder
RUN apt update && apt install -y ca-certificates pkg-config libssl-dev libclang-dev
WORKDIR /src
COPY . /src

RUN mkdir /out
RUN rustup component add rustfmt
RUN cargo build --release --manifest-path ./Cargo.toml
RUN cp target/release/stacks-devnet-api /out

FROM gcr.io/distroless/cc-debian11
COPY --from=builder /out/ /bin/

CMD ["stacks-devnet-api"]