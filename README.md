# TV Server

## Introduction

TV Server 
- provides ad free viewing of videos published on the public internet.
- connects Youtube and PirateBay to your TV and lets you control them with your mobile phone.
- is a thin wrapper around [FFmpeg](https://ffmpeg.org/), 
[yt-dlp](https://github.com/yt-dlp/yt-dlp) and [Transmission](https://transmissionbt.com/)

Architecturally, it consists of 3 components:

- The remote control, which is a web app that acts as a remote control
- The player, which is a web app that runs on the TV
- The server, which is a daemon that hosts the player and remote control apps and handles downloading 
and streaming movies; 

It tested on macOS and Ubuntu Linux.

## Setup

The easiest way to get started is with docker compose:

```shell
$ docker compose up
```
NB YouTube search will not work until a key for the Google API is provided through the 
`GOOGLE_KEY` environment variable - see below.

## Configuration

| Environment Variable | Description                                                                        |
|----------------------|------------------------------------------------------------------------------------|
| GOOGLE_KEY           | *A key to use with the Google API, see below for instructions for obtaining a key. |
| MOVIE_DIR            | *The directory where movies will be stored.                                        |
| CLIENT_DIR           | The directory contain the client apps, defaults to `clients`                       |
| TRANSMISSION_URL     | URL to access the Transmission HTTP interface                                      |
| TRANSMISSION_USER    | Username to access the Transmission HTTP interface                                 |
| TRANSMISSION_PWD     | Password to access the Transmission HTTP interface                                 |
| PIRATE_BAY_PROXY_URL | URL of the Pirate Bay proxy site to search, defaults to `https://thehiddenbay.com` |
| TORRENT_DIR          | The directory where torrent files are saved.                                       |

`*` Required

## Obtaining a Google API Key

A Google account is required to create an API Key. Instructions for obtaining the key are
described here: https://support.google.com/googleapi/answer/6158862?hl=en

## Clients

For simplicity, compiled versions of the apps are available in the `clients` directory.

The full source for the clients are available as submodules that can be downloaded using
the following commands:

```shell
$ git submodule init
$ git submodule update
```

Any changes made to the clients can be compiled and copied to the `clients` directory by executing the
following in client source directory:

```shell
client_src/tvremote $ yarn run export
```

## File Formats

All AVI files a re-formatted to MP4, as few, if any, browsers support AVI.

## VLC Player

VLC has support for a very broad range of video and audio codecs.
As a fallback, can player files with VLC on the monitor/TV connected to the box running the 
server. 
