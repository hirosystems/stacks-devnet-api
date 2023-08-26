FROM rust:bullseye as builder
WORKDIR /src
COPY . /src

RUN mkdir /out
RUN cargo build --release --manifest-path ./Cargo.toml
RUN cp target/release/stacks-devnet-api /out

FROM gcr.io/distroless/cc
COPY --from=builder /out/ /bin/

CMD ["stacks-devnet-api"]