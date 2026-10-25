"""Verify the library/Save window is visible when returning through Recent apps.

Run after the on-device tests, with MainActivity open. This catches the blank
test-helper card which can otherwise be mistaken for the real application.
"""
import argparse
import re
import struct
import subprocess
import time
import xml.etree.ElementTree as ET

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--adb', default='adb')
parser.add_argument('--serial', default='emulator-5554')
args = parser.parse_args()
command = [args.adb, '-s', args.serial]


def adb(*arguments):
    return subprocess.check_output(command + list(arguments), timeout=15).decode().strip()


def hierarchy():
    adb('shell', 'uiautomator', 'dump', '/data/local/tmp/lov-recents-check.xml')
    return ET.fromstring(adb('exec-out', 'cat', '/data/local/tmp/lov-recents-check.xml'))


try:
    # Dump only the native launcher tree. Traversing a large WebView accessibility
    # tree is unnecessary here and can exhaust memory on a small emulator.
    adb('shell', 'input', 'keyevent', 'KEYCODE_HOME')
    adb('shell', 'input', 'keyevent', 'KEYCODE_APP_SWITCH')
    root = hierarchy()
    cards = [n for n in root.iter('node') if n.get('resource-id', '').endswith('/snapshot')]
    helpers = [n for n in cards if 'lots_of_videos.test' in n.get('content-desc', '')
               or 'Lots of Videos tests' in n.get('content-desc', '')]
    apps = [n for n in cards if n.get('content-desc') == 'lots-of-videos']
    target = (helpers or apps or [None])[0]
    assert target is not None, 'Open MainActivity before checking Recent apps'
    x1, y1, x2, y2 = map(int, re.findall(r'\d+', target.get('bounds')))
    adb('shell', 'input', 'tap', str((x1 + x2) // 2), str((y1 + y2) // 2))
    time.sleep(1)
    focus = next(line.strip() for line in adb('shell', 'dumpsys', 'window').splitlines()
                 if 'mCurrentFocus=' in line)
    print('Selected:', target.get('content-desc'))
    print(focus)
    assert not helpers, 'A blank instrumentation helper is exposed in Recent apps'
    assert 'EmptyActivity' not in focus, 'Returning to the app shows a blank testing window'
    assert 'com.abamaxa.lots_of_videos/' in focus or 'documentsui' in focus, 'Unexpected foreground app'
    # Check the rendered screen as well, including WebViews which do not expose
    # their DOM through accessibility. Ignore the status/navigation bar regions.
    deadline = time.monotonic() + 15
    while True:
        raw = subprocess.check_output(command + ['exec-out', 'screencap'], timeout=15)
        width, height, pixel_format = struct.unpack_from('<III', raw)
        assert pixel_format in (1, 2), f'Unexpected screen pixel format {pixel_format}'
        pixels = raw[-width * height * 4:]
        offsets = [4 * (y * width + x) for y in range(150, height - 150, 16)
                   for x in range(0, width, 16)]
        nonwhite = sum(min(pixels[offset:offset + 3]) < 240 for offset in offsets) / len(offsets)
        if nonwhite > 0.001 or time.monotonic() >= deadline:
            break
        time.sleep(0.5)
    print(f'Rendered nonwhite content: {nonwhite:.2%}')
    assert nonwhite > 0.001, 'Returning to the app leaves the rendered screen all white'
    print('PASS: Recent apps returns to visible application content')
finally:
    adb('shell', 'rm', '-f', '/data/local/tmp/lov-recents-check.xml')
