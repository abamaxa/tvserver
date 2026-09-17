FROM rust:1-bullseye as builder
WORKDIR /usr/src/tvserver
COPY . .
COPY client /var/www/client
RUN cargo install -j -1 --debug --path .

FROM python:3.13-slim-bookworm
COPY --from=node:22-bookworm-slim /usr/local/bin/node /usr/local/bin/node
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg ca-certificates && rm -rf /var/lib/apt/lists/*
RUN node --version && python3 -m pip install --no-cache-dir --upgrade "yt-dlp[default]"

COPY --from=builder /usr/local/cargo/bin/tvserver /usr/local/bin/tvserver
COPY --from=builder /var/www/client /var/www/client
COPY --from=builder /usr/src/tvserver/migrations /usr/src/tvserver/migrations

CMD ["tvserver"]
