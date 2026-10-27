#!/usr/bin/env bash
set -euo pipefail

# Green/tone and red/silence share timestamps, alternating every two seconds.
# Long GOPs exercise seeking; the AV1/AAC variant matches YouTube MP4 codecs.
fixture_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../gen/android/app/src/androidTest/assets" && pwd)"
video="color=c=red:s=320x180:r=30:d=28,drawbox=x=0:y=0:w=iw:h=ih:color=lime:t=fill:enable='lt(mod(t,4),2)'"
audio='aevalsrc=if(lt(mod(t\,4)\,2)\,0.3*sin(2*PI*880*t)\,0):s=48000:d=28'

ffmpeg -hide_banner -loglevel error -f lavfi -i "$video" -f lavfi -i "$audio" \
  -c:v libx264 -preset fast -crf 24 -g 120 -bf 3 -pix_fmt yuv420p \
  -c:a aac -b:a 96k -movflags +faststart -y "$fixture_dir/native-av-sync-test.mp4"

ffmpeg -hide_banner -loglevel error -i "$fixture_dir/native-av-sync-test.mp4" \
  -c:v libsvtav1 -preset 12 -crf 45 -r 25 -g 100 \
  -c:a aac -ar 44100 -b:a 96k -movflags +faststart -y "$fixture_dir/native-av1-sync-test.mp4"
