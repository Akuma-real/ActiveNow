ARG RUST_VERSION=1.82
ARG ALPINE_VERSION=3.20

FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS builder
WORKDIR /app
RUN apk add --no-cache musl-dev build-base

# 预拷贝清单加速依赖缓存
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main(){}" > src/main.rs && cargo build --release || true

# 拷贝实际源码并构建
COPY src ./src
RUN cargo build --release && strip target/release/activenow || true

# --- Runtime ---
FROM alpine:${ALPINE_VERSION}
ARG VERSION=dev
ARG GIT_SHA=unknown
ARG BUILD_TIME=unknown

LABEL org.opencontainers.image.title="activenow" \
      org.opencontainers.image.description="Room-level online presence via WebSocket (Rust)" \
      org.opencontainers.image.version=$VERSION \
      org.opencontainers.image.revision=$GIT_SHA \
      org.opencontainers.image.created=$BUILD_TIME

RUN adduser -D -u 10001 appuser

COPY --from=builder /app/target/release/activenow /usr/local/bin/activenow

ENV RUST_LOG=info \
    PORT=8080
EXPOSE 8080
USER 10001
ENTRYPOINT ["/usr/local/bin/activenow"]
