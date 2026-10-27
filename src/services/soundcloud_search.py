"""Keep availability metadata that yt-dlp's flat search results discard."""

import json
import sys


def search_tracks(query, limit):
    from yt_dlp import YoutubeDL
    from yt_dlp.extractor.soundcloud import SoundcloudSearchIE

    with YoutubeDL({
        "quiet": True,
        "no_warnings": True,
        "socket_timeout": 15,
        "extractor_retries": 1,
    }) as downloader:
        # Reuse yt-dlp's client ID discovery, cache and API error handling. A
        # single search request contains the policy and every stream's flags;
        # resolving each track's media URLs would be slower and use rate limits.
        extractor = SoundcloudSearchIE(downloader)
        extractor.initialize()
        response = extractor._call_api(
            extractor._API_V2_BASE + "search/tracks",
            query,
            query={"q": query, "limit": limit, "linked_partitioning": 1, "offset": 0},
            headers=extractor._HEADERS,
        )
        if not isinstance(response, dict) or not isinstance(response.get("collection"), list):
            raise ValueError("SoundCloud returned an invalid search response")
        return {"entries": response["collection"]}


def main():
    package_dir, query, limit = sys.argv[1:]
    sys.path.insert(0, package_dir)
    json.dump(search_tracks(query, int(limit)), sys.stdout, ensure_ascii=False)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
