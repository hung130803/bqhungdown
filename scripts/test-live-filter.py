"""Verify production filter semantics with yt-dlp and its actual CLI (offline)."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from yt_dlp.utils import match_filter_func

root = Path(__file__).resolve().parents[1]
source = (root / 'src-tauri/src/args_builder.rs').read_text(encoding='utf-8')
rule = re.search(r'const LIVE_FILTER: &str = "([^"]+)"', source)[1]
check = match_filter_func(rule)
cases = [(dict(live_status=s), blocked) for s, blocked in
         [('is_live', True), ('is_upcoming', True), ('post_live', True),
          ('was_live', False), ('not_live', False)]]
cases += [({}, False), ({'is_live': True}, True), ({'is_live': False}, False)]
for fields, blocked in cases:
    assert (check(dict(title='live in downtown inwood', **fields)) is not None) == blocked, fields

binary = os.environ.get('BQD_TEST_YTDLP')
command = [binary] if binary else [sys.executable, '-m', 'yt_dlp']
with tempfile.TemporaryDirectory(dir=root / 'src-tauri/target') as tmp:
    for fields, blocked in cases:
        fixture = Path(tmp) / 'info.json'
        fixture.write_text(json.dumps(dict(id='fixture', title='live in downtown inwood',
            url='http://127.0.0.1:1/never-request', ext='mp4', extractor='generic',
            extractor_key='Generic', **fields)), encoding='utf-8')
        result = subprocess.run(command + ['--ignore-config', '--no-update',
            '--simulate', '--match-filters', rule, '--no-wait-for-video',
            '--load-info-json', str(fixture)], capture_output=True, text=True, timeout=20)
        assert result.returncode == 0, result.stderr
        rejected = 'does not pass filter' in result.stdout + result.stderr
        assert rejected == blocked, (fields, result.stdout, result.stderr)
print('PASS: 8 live/status cases through filter and real yt-dlp CLI; no media requested')
