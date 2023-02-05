FROM rust:1.67 as builder
WORKDIR /usr/src/tvserver
COPY . .
COPY client /var/www/client
COPY test.mp4 /var/www/
RUN cargo install -j -1 --path .

FROM debian:bullseye-slim
# RUN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/tvserver /usr/local/bin/tvserver
COPY --from=builder /var/www/client /var/www/client
COPY --from=builder /var/www/test.mp4 /var/www/test.mp4

ENV DATABASE_URL=:memory:
ENV DISABLE_PLAYER=true
ENV MOVIE_DIR=/movies
ENV CLIENT_DIR=/var/www/client

CMD ["tvserver"]