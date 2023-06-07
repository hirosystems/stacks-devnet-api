FROM arm64v8/rust:1.67 as builder

WORKDIR ./
COPY . ./

RUN cargo build --manifest-path ./Cargo.toml

FROM gcr.io/distroless/cc
COPY --from=builder target/debug/stacks-devnet-api /

ENTRYPOINT ["./stacks-devnet-api"]