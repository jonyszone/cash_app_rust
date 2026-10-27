FROM rust:1-slim-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Build dependencies first so they are cached until Cargo.toml/Cargo.lock change.
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release && rm -rf src
COPY migrations ./migrations
COPY src ./src
# Touch so cargo rebuilds the crate over the dummy sources.
RUN touch src/main.rs src/lib.rs && cargo build --release

FROM debian:bookworm-slim
# TLS roots are bundled into the binary (reqwest rustls-tls), so no CA package is needed.
RUN useradd --system --uid 10001 --home /data app && mkdir /data && chown app /data
COPY --from=build /app/target/release/cash_app_backend /usr/local/bin/cash_app_backend
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod 755 /usr/local/bin/docker-entrypoint.sh
# Starts as root only to fix /data ownership; the server itself runs as `app`.
ENV DATABASE_URL=sqlite:///data/cash_app.db \
    BIND_ADDR=0.0.0.0:8000
VOLUME /data
EXPOSE 8000
ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["cash_app_backend"]
