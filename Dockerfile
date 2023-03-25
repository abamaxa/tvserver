FROM rust:1-bullseye as builder
WORKDIR /usr/src/tvserver
COPY . .
COPY client /var/www/client
RUN cargo install -j -1 --debug --path .

FROM debian:bullseye
RUN apt-get update && apt-get install -y ffmpeg python3 python3-pip && rm -rf /var/lib/apt/lists/*
RUN pip3 install -U yt-dlp

COPY --from=builder /usr/local/cargo/bin/tvserver /usr/local/bin/tvserver
COPY --from=builder /var/www/client /var/www/client
COPY --from=builder /usr/src/tvserver/migrations /usr/src/tvserver/migrations

CMD ["tvserver"]