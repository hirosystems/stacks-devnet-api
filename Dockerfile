FROM arm64v8/rust:1.67 as builder

WORKDIR ./
COPY . ./

RUN cargo build --release --manifest-path ./Cargo.toml

FROM gcr.io/distroless/cc
COPY --from=builder target/release/stacks-devnet-api /

ENTRYPOINT ["./stacks-devnet-api"]