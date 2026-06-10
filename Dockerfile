# Production image: builds the wasm client + release server, ships both.
FROM rust:1 AS builder
WORKDIR /app
RUN rustup target add wasm32-unknown-unknown
COPY . .
RUN cargo build --release -p ferraria-server \
    && cargo build --release -p ferraria-client --target wasm32-unknown-unknown \
    && cp target/wasm32-unknown-unknown/release/ferraria-client.wasm web/ferraria-client.wasm

FROM debian:trixie-slim
WORKDIR /app
COPY --from=builder /app/target/release/ferraria-server /app/ferraria-server
COPY --from=builder /app/web /app/web
ENV WEB_DIR=/app/web
ENV PORT=3000
EXPOSE 3000
CMD ["/app/ferraria-server"]
