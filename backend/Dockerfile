# 多阶段构建：把 SQLite（bundled）静态编译进二进制，运行镜像使用 scratch。
FROM rust:1.88-alpine AS builder

RUN apk add --no-cache build-base
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src src
COPY migrations migrations
RUN cargo build --release --locked

FROM scratch

COPY --from=builder /app/target/release/diary /diary

ENV DIARY_BIND=0.0.0.0:8787 \
    DIARY_DB_PATH=/data/diary.db \
    DIARY_SECURE_COOKIE=true \
    DIARY_RUST_LOG=info

VOLUME ["/data"]
EXPOSE 8787
ENTRYPOINT ["/diary"]
