FROM rust:1.67 as builder
WORKDIR /usr/src/tvserver
COPY . .
COPY client /var/www/client
RUN cargo install -j 5 --debug --path .

FROM ubuntu:jammy
RUN apt-get update && apt-get install -y ffmpeg python3 python3-pip && rm -rf /var/lib/apt/lists/*
RUN pip3 install -U yt-dlp

COPY --from=builder /usr/local/cargo/bin/tvserver /usr/local/bin/tvserver
COPY --from=builder /var/www/client /var/www/client
COPY --from=builder /usr/src/tvserver/migrations /usr/src/tvserver/migrations

ENV DATABASE_URL=:memory:
ENV DISABLE_PLAYER=true
ENV MOVIE_DIR=/movies
ENV CLIENT_DIR=/var/www/client
ENV DATABASE_MIGRATION_DIR=/usr/src/tvserver/migrations

CMD ["tvserver"]