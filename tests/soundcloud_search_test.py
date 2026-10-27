"""Exercise the metadata bridge with the yt-dlp version shipped on Android."""

import contextlib
import io
import json
from pathlib import Path
import runpy
import sys
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
BUNDLED_YTDLP = ROOT / "gen/android/app/src/main/assets/ytdlp/yt-dlp"
sys.path.insert(0, str(BUNDLED_YTDLP))

from yt_dlp.extractor.soundcloud import SoundcloudSearchIE


BRIDGE = runpy.run_path(str(ROOT / "src/services/soundcloud_search.py"))


class SoundcloudSearchTest(unittest.TestCase):
    def setUp(self):
        # Client discovery is covered by live checks. Unit fixtures never make
        # network calls, including when the developer has no cached client ID.
        self.initialization = patch.object(SoundcloudSearchIE, "initialize")
        self.initialization.start()
        self.addCleanup(self.initialization.stop)
        api_patch = patch.object(SoundcloudSearchIE, "_call_api")
        self.api = api_patch.start()
        self.addCleanup(api_patch.stop)

    def test_main_preserves_availability_and_special_characters(self):
        tracks = [
            {"title": "Café", "policy": "MONETIZE", "streamable": True,
             "media": {"transcodings": [{"snipped": False}]}},
            {"title": "Preview", "policy": "SNIP", "streamable": True,
             "media": {"transcodings": [{"snipped": True}]}},
            {"title": "Blocked", "policy": "BLOCK", "streamable": False},
        ]
        self.api.return_value = {"collection": tracks}
        query = "jazz & café + 100%"
        output = io.StringIO()
        with patch.object(sys, "argv", ["bridge", str(BUNDLED_YTDLP), query, "100"]), \
                patch.object(sys, "path", list(sys.path)), contextlib.redirect_stdout(output):
            BRIDGE["main"]()
        self.assertEqual(json.loads(output.getvalue()), {"entries": tracks})
        self.api.assert_called_once()
        self.assertEqual(self.api.call_args.args[0], "https://api-v2.soundcloud.com/search/tracks")
        self.assertEqual(self.api.call_args.kwargs["query"]["q"], query)
        self.assertEqual(self.api.call_args.kwargs["query"]["limit"], 100)

    def test_empty_search_is_a_valid_empty_result(self):
        self.api.return_value = {"collection": []}
        self.assertEqual(BRIDGE["search_tracks"]("nothing", 100), {"entries": []})

    def test_malformed_collection_is_an_error(self):
        for response in [None, {}, {"collection": None}, {"collection": "invalid"}]:
            with self.subTest(response=response):
                self.api.return_value = response
                with self.assertRaisesRegex(ValueError, "invalid search response"):
                    BRIDGE["search_tracks"]("query", 100)

    def test_api_failure_is_not_hidden_as_empty_results(self):
        self.api.side_effect = RuntimeError("SoundCloud unavailable")
        with self.assertRaisesRegex(RuntimeError, "SoundCloud unavailable"):
            BRIDGE["search_tracks"]("query", 100)


if __name__ == "__main__":
    unittest.main()
