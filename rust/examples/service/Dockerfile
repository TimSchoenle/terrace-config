# The orders example, built into an image that publishes its configuration contract.
#
# Built from `rust/` as the context:
#
#   docker build -f examples/service/Dockerfile -t orders-service .
#
# The service-specific part is three lines: the `RUN` that renders the contract with the example's
# own generator, the `COPY` that embeds it at the path the labels name, and the label region, which
# `terrace-contract render --format dockerfile --write examples/service/Dockerfile contract.json`
# fills and `--check` keeps honest. Everything else is an ordinary Rust image.

FROM rust:1.99.1-slim-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --example service \
 && cargo run --release --example service-contract --features schema-cli -- --format contract > /contract.json

FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /src/target/release/examples/service /usr/local/bin/orders-service
COPY --from=build /contract.json /config/contract.json
# terrace-config:labels:begin
LABEL dev.terrace.config.contract.version="1" \
      dev.terrace.config.contract.path="/config/contract.json" \
      dev.terrace.config.prefix="ORDERS_"
# terrace-config:labels:end
ENTRYPOINT ["/usr/local/bin/orders-service"]
