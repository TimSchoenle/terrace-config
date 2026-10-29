# The orders service, configured through Spring Boot, with its contract inside.
#
# Build from java/, which holds the Gradle build this module belongs to:
#
#     docker build -f terrace-config-example-spring-service/Dockerfile \
#         --build-arg TERRACE_CONTRACT_REVISION="$(git rev-parse HEAD)" -t orders-service .
#
# The build stage runs `terraceContract`, which emits the contract from the configuration types,
# stamps it and conforms it, so an image whose contract does not conform is never built.

FROM ghcr.io/timschoenle/terrace-contract:0.12 AS terrace-contract

FROM eclipse-temurin:25.0.4.1_1-jdk-noble@sha256:a9c5ddd386f629518bd500ecfabf7d1e5d32d7ffed735d5c3b1604ae92c830f4 AS build
COPY --from=terrace-contract /terrace-contract /usr/local/bin/terrace-contract
ENV TERRACE_CONTRACT_BIN=/usr/local/bin/terrace-contract
# Read by `terraceContract` as app.revision and app.created; unset leaves them out.
ARG TERRACE_CONTRACT_REVISION
ARG TERRACE_CONTRACT_CREATED
WORKDIR /src
COPY . .
RUN ./gradlew --no-daemon --console=plain :terrace-config-example-spring-service:installDist :terrace-config-example-spring-service:terraceContract

FROM eclipse-temurin:25.0.4.1_1-jre-noble@sha256:ce8543ddd3070ccbeaac2106a74c9387ddeb8d47a1c43348345715c1ef245c7f
COPY --from=build /src/terrace-config-example-spring-service/build/install/terrace-config-example-spring-service /app
COPY --from=build /src/terrace-config-example-spring-service/build/terrace/contract.json /config/contract.json
# terrace-config:labels:begin
LABEL dev.terrace.config.contract.version="1" \
      dev.terrace.config.contract.path="/config/contract.json" \
      dev.terrace.config.prefix="ORDERS_"
# terrace-config:labels:end
USER 65532:65532
ENTRYPOINT ["/app/bin/terrace-config-example-spring-service"]
