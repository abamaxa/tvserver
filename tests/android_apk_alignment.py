#!/usr/bin/env python3
"""Check the APK's actual 64-bit ELF load segments for Android 16 KB support.

Usage: python3 tests/android_apk_alignment.py path/to/app.apk
The downloader SDK's *.zip.so files are data archives, not loadable libraries.
"""
import argparse
import struct
import sys
import zipfile

PAGE_SIZE = 16384


def check_apk(apk):
    failures = []
    checked = 0
    with zipfile.ZipFile(apk) as archive:
        for name in sorted(archive.namelist()):
            if not name.startswith(("lib/arm64-v8a/", "lib/x86_64/")) or not name.endswith(".so"):
                continue
            with archive.open(name) as library:
                header = library.read(64)
                if header[:4] != b"\x7fELF":
                    if name.endswith(".zip.so") and header[:4] == b"PK\x03\x04":
                        print(f"DATA {name} (SDK archive, not executable code)")
                        continue
                    failures.append(f"{name}: expected an ELF shared library")
                    continue
                if header[4:6] != b"\x02\x01":
                    failures.append(f"{name}: expected little-endian ELF64")
                    continue
                offset = struct.unpack_from("<Q", header, 32)[0]
                entry_size, count = struct.unpack_from("<HH", header, 54)
                library.seek(offset)
                segments = [struct.unpack("<IIQQQQQQ", library.read(entry_size)[:56])
                            for _ in range(count)]
            loads = [segment for segment in segments if segment[0] == 1]
            bad = [segment for segment in loads if segment[7] < PAGE_SIZE
                   or (segment[3] - segment[2]) % PAGE_SIZE]
            checked += 1
            if not loads or bad:
                failures.append(f"{name}: LOAD segments not 16 KB aligned "
                                f"(alignments {[segment[7] for segment in loads]})")
                print(f"FAIL {failures[-1]}")
            else:
                print(f"PASS {name}")
    if not checked:
        failures.append("APK contains no 64-bit ELF libraries")
    if failures:
        print(f"16 KB alignment FAILED: {len(failures)} issue(s)", file=sys.stderr)
        return 1
    print(f"16 KB alignment passed for all {checked} native libraries")
    return 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("apk")
    sys.exit(check_apk(parser.parse_args().apk))
