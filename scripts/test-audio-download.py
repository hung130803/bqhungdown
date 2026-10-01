"""Offline regression: real yt-dlp HTTP downloader against a bounded-range server.

Run: python scripts/test-audio-download.py
No YouTube requests, account cookies, or application data used.
"""
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import shutil
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

from yt_dlp import YoutubeDL

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    'bqd_audio', ROOT / 'src-tauri/resources/audio/yt_dlp_plugins/postprocessor/bqd_audio.py')
plugin = importlib.util.module_from_spec(spec)
spec.loader.exec_module(plugin)
DATA = bytes(range(256)) * 24001  # > two chunks, final chunk not aligned


class Server(BaseHTTPRequestHandler):
    requests_seen = []

    def do_GET(self):
        value = self.headers.get('Range', '')
        type(self).requests_seen.append(value)
        match = re.fullmatch(r'bytes=(\d+)-(\d+)', value)
        if not match:
            self.send_error(400, 'Bounded range required')
            return
        start, end = map(int, match.groups())
        if end - start + 1 > 2 * 1024 * 1024:
            self.send_error(400, 'Range too large')
            return
        end = min(end, len(DATA) - 1)
        self.send_response(206)
        self.send_header('Content-Range', f'bytes {start}-{end}/{len(DATA)}')
        self.send_header('Content-Length', str(end - start + 1))
        self.end_headers()
        self.wfile.write(DATA[start:end + 1])

    def log_message(self, *_):
        pass


class AudioTests(unittest.TestCase):
    def audio(self, **changes):
        return dict(dict(url='https://rr1.googlevideo.com/videoplayback', protocol='https',
                         vcodec='none', acodec='mp4a.40.2'), **changes)

    def test_scope(self):
        self.assertTrue(plugin.is_youtube_audio(self.audio()))
        for change in [dict(vcodec='avc1'), dict(is_live=True), dict(protocol='m3u8_native'),
                       dict(url='https://googlevideo.com.attacker.test/a'),
                       dict(url='https://other.test/a'), dict(acodec='none'),
                       dict(requested_formats=[{}])]:
            self.assertFalse(plugin.is_youtube_audio(self.audio(**change)))

    def test_restore_params_after_failure_and_preserve_video(self):
        with YoutubeDL({'quiet': True, 'external_downloader': {'default': 'aria2c'}}) as ydl:
            params = ydl.params
            seen = []
            def dl(name, info, **kwargs):
                seen.append(ydl.params.copy())
                raise RuntimeError('network disconnected')
            ydl.dl = dl
            pp = plugin.BqdAudioPP(ydl)
            pp.run({})
            wrapped = ydl.dl
            pp.run({})
            self.assertIs(ydl.dl, wrapped)
            for info in [self.audio(), self.audio(vcodec='avc1')]:
                with self.assertRaises(RuntimeError):
                    ydl.dl('unused', info)
                self.assertIs(ydl.params, params)
            self.assertEqual(seen[0]['external_downloader']['default'], 'native')
            self.assertEqual(seen[1]['external_downloader']['default'], 'aria2c')
            self.assertNotIn('http_chunk_size', seen[1])

    def test_real_http_chunks_and_resume(self):
        server = ThreadingHTTPServer(('127.0.0.1', 0), Server)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory(dir=ROOT / 'src-tauri/target') as tmp:
                for resume in ['fresh', 'native', 'aria2']:
                    Server.requests_seen.clear()
                    path = Path(tmp) / f'{resume}.m4a'
                    offset = 123457 if resume == 'native' else 0
                    if resume == 'native':
                        Path(str(path) + '.part').write_bytes(DATA[:offset])
                    elif resume == 'aria2':
                        Path(str(path) + '.part').write_bytes(b'\x00' * len(DATA))
                        Path(str(path) + '.part.aria2').write_bytes(b'old sparse download')
                    with YoutubeDL({'quiet': True, 'noprogress': True, 'proxy': '',
                                    'external_downloader': {'default': 'aria2c'}}) as ydl:
                        original_dl = ydl.dl
                        # Classify a real YouTube URL, then redirect only the
                        # transport to our local server. Production dl is real.
                        def local_dl(name, info, **kwargs):
                            info = dict(info, url=f'http://127.0.0.1:{server.server_port}/audio')
                            return original_dl(name, info, **kwargs)
                        ydl.dl = local_dl
                        events = []
                        ydl.add_progress_hook(lambda p: events.append(p['status']))
                        plugin.BqdAudioPP(ydl).run({})
                        result = ydl.dl(str(path), self.audio())
                        self.assertTrue(result[0])
                        self.assertEqual(hashlib.sha256(path.read_bytes()).digest(),
                                         hashlib.sha256(DATA).digest())
                        self.assertGreaterEqual(len(Server.requests_seen), 3)
                        self.assertTrue(Server.requests_seen[0].startswith(f'bytes={offset}-'))
                        self.assertIn('finished', events)
                        self.assertFalse(Path(str(path) + '.part').exists())
        finally:
            server.shutdown()
            server.server_close()

    def test_cli_plugin_loading_and_native_selection(self):
        server = ThreadingHTTPServer(('127.0.0.1', 0), Server)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            with tempfile.TemporaryDirectory(dir=ROOT / 'src-tauri/target') as tmp:
                path = Path(tmp)
                info = self.audio(protocol='http', url='http://rr1.googlevideo.com/audio')
                info.update(id='offline-audio', title='Offline audio', ext='m4a',
                            extractor='generic', extractor_key='Generic',
                            webpage_url='http://rr1.googlevideo.com/audio')
                fixture = path / 'info.json'
                fixture.write_text(json.dumps(info), encoding='utf-8')
                import sys
                binary = os.environ.get('BQD_TEST_YTDLP')
                command = [binary] if binary else [sys.executable, '-m', 'yt_dlp']
                command += ['--ignore-config', '--no-update',
                            '--plugin-dirs', str(ROOT / 'src-tauri/resources'),
                            '--use-postprocessor', 'BqdAudio:when=before_dl',
                            '--proxy', f'http://127.0.0.1:{server.server_port}',
                            '--downloader', 'aria2c', '--no-progress',
                            '--load-info-json', str(fixture), '-o', str(path / 'out.m4a')]
                result = subprocess.run(command, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                self.assertEqual((path / 'out.m4a').read_bytes(), DATA)
        finally:
            server.shutdown()
            server.server_close()

    def test_video_audio_merge_through_real_cli(self):
        import sys
        binary = os.environ.get('BQD_TEST_YTDLP')
        ffmpeg = shutil.which('ffmpeg')
        ffprobe = shutil.which('ffprobe')
        aria = str(Path(binary).with_name('aria2c.exe')) if binary else shutil.which('aria2c')
        if not (ffmpeg and ffprobe and aria and Path(aria).is_file()):
            self.skipTest('ffmpeg, ffprobe and aria2c required for merge test')
        with tempfile.TemporaryDirectory(dir=ROOT / 'src-tauri/target') as tmp:
            tmp = Path(tmp)
            for kind, source, codec, ext in [('video', 'color=c=black:s=160x90:d=1',
                                            ['-an', '-c:v', 'libx264'], 'mp4'),
                                           ('audio', 'sine=frequency=1000:duration=1',
                                            ['-c:a', 'aac'], 'm4a')]:
                subprocess.run([ffmpeg, '-v', 'error', '-f', 'lavfi', '-i', source,
                                *codec, str(tmp / f'{kind}.{ext}')], check=True,
                               capture_output=True, timeout=15)
            media = {'/video': (tmp / 'video.mp4').read_bytes(),
                     '/audio': (tmp / 'audio.m4a').read_bytes()}
            requests = []

            class MediaServer(BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_GET(self):
                    path = urlsplit(self.path).path
                    data = media[path]
                    value = self.headers.get('Range', '')
                    requests.append((path, value))
                    match = re.fullmatch(r'bytes=(\d+)-(\d*)', value)
                    if path == '/audio' and not re.fullmatch(r'bytes=\d+-\d+', value):
                        self.send_error(400, 'Audio must use bounded requests')
                        return
                    if match:
                        start = int(match[1])
                        end = min(int(match[2]) if match[2] else len(data)-1, len(data)-1)
                        self.send_response(206)
                        self.send_header('Content-Range', f'bytes {start}-{end}/{len(data)}')
                        data = data[start:end+1]
                    else:
                        self.send_response(200)
                    self.send_header('Content-Length', str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)

            server = ThreadingHTTPServer(('127.0.0.1', 0), MediaServer)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            try:
                info = dict(id='merge-probe', title='Merge probe', extractor='generic',
                            extractor_key='Generic', webpage_url='http://rr1.googlevideo.com/watch',
                            formats=[dict(format_id='137', url='http://rr1.googlevideo.com/video',
                                          protocol='http', ext='mp4', vcodec='avc1', acodec='none'),
                                     dict(self.audio(protocol='http', url='http://rr1.googlevideo.com/audio'),
                                          format_id='140', ext='m4a')])
                fixture = tmp / 'info.json'
                fixture.write_text(json.dumps(info), encoding='utf-8')
                command = [binary] if binary else [sys.executable, '-m', 'yt_dlp']
                command += ['--ignore-config', '--no-update', '--no-progress',
                            '--plugin-dirs', str(ROOT / 'src-tauri/resources'),
                            '--use-postprocessor', 'BqdAudio:when=before_dl',
                            '--proxy', f'http://127.0.0.1:{server.server_port}',
                            '--downloader', aria, '--ffmpeg-location', ffmpeg,
                            '--load-info-json', str(fixture), '-f', '137+140',
                            '--merge-output-format', 'mp4', '-o', str(tmp / 'merged.%(ext)s')]
                result = subprocess.run(command, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                check = subprocess.run([ffprobe, '-v', 'error', '-show_entries',
                                        'stream=codec_type', '-of', 'json', str(tmp / 'merged.mp4')],
                                       capture_output=True, text=True, check=True, timeout=10)
                self.assertEqual(sorted(s['codec_type'] for s in json.loads(check.stdout)['streams']),
                                 ['audio', 'video'])
                self.assertTrue(any(p == '/audio' and v for p, v in requests))
                self.assertTrue(any(p == '/video' for p, v in requests))
            finally:
                server.shutdown()
                server.server_close()


if __name__ == '__main__':
    unittest.main()
