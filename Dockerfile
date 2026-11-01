FROM node:22-bookworm-slim AS web
WORKDIR /app
COPY package*.json ./
COPY pitwall-web/package.json pitwall-web/package.json
RUN npm ci
COPY pitwall-web pitwall-web
COPY crates/pitwall-core/bindings crates/pitwall-core/bindings
RUN npm run build

FROM rust:1.99-bookworm AS server
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
RUN cargo build --locked --release -p pitwall-api

FROM python:3.13-slim-bookworm AS inference
RUN pip install --no-cache-dir onnxruntime==1.31.1
RUN mkdir /runtime && cp /usr/local/lib/python3.13/site-packages/onnxruntime/capi/libonnxruntime.so* /runtime/

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl libgomp1 libstdc++6 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server /app/target/release/pitwall-api /usr/local/bin/agon
COPY --from=web /app/pitwall-web/dist pitwall-web/dist
COPY --from=inference /runtime /opt/onnxruntime
COPY models models
RUN mkdir -p /app/data && chown -R 10001:10001 /app/data
USER 10001:10001
ENV AGON_BIND=0.0.0.0:3001 ONNXRUNTIME_DYLIB_PATH=/opt/onnxruntime/libonnxruntime.so.1.31.1
EXPOSE 3001
HEALTHCHECK --interval=15s --timeout=3s CMD curl -fsS http://127.0.0.1:3001/health || exit 1
CMD ["agon"]
