import unittest

from android_instrumentation import instrumentation_succeeded


class InstrumentationResultTest(unittest.TestCase):
    def test_completed_shard_passes_including_skipped_tests(self):
        self.assertTrue(instrumentation_succeeded(
            "INSTRUMENTATION_STATUS_CODE: -3\nOK (14 tests)\nINSTRUMENTATION_CODE: -1\n"))

    def test_adb_success_does_not_hide_failed_or_crashed_tests(self):
        for output in (
            "FAILURES!!!\nTests run: 14, Failures: 1\nINSTRUMENTATION_CODE: -1\n",
            "INSTRUMENTATION_RESULT: shortMsg=Process crashed.\nINSTRUMENTATION_CODE: 0\n",
            "OK (14 tests)\nINSTRUMENTATION_CODE: 0\n",
            "INSTRUMENTATION_STATUS_CODE: -2\nOK (14 tests)\nINSTRUMENTATION_CODE: -1\n",
            "OK (0 tests)\nINSTRUMENTATION_CODE: -1\n",
            "INSTRUMENTATION_STATUS_CODE: 1\n",
        ):
            with self.subTest(output=output):
                self.assertFalse(instrumentation_succeeded(output))


if __name__ == "__main__":
    unittest.main()
