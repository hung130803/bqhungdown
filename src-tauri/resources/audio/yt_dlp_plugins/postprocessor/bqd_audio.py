"""Keep video downloads unchanged; use bounded HTTP ranges for YouTube audio."""
from urllib.parse import urlsplit
from pathlib import Path

from yt_dlp.postprocessor.common import PostProcessor


def is_youtube_audio(info):
    host = (urlsplit(info.get('url') or '').hostname or '').lower()
    return (
        host.endswith('.googlevideo.com')
        and info.get('protocol') in ('http', 'https')
        and info.get('vcodec') == 'none'
        and info.get('acodec') not in (None, 'none')
        and not info.get('is_live')
        and not info.get('requested_formats')
    )


class BqdAudioPP(PostProcessor):
    def run(self, info):
        ydl = self._downloader
        if getattr(ydl, '_bqd_audio_installed', False):
            return [], info
        original_dl = ydl.dl

        def download(name, stream, subtitle=False, test=False):
            if subtitle or test or not is_youtube_audio(stream):
                return original_dl(name, stream, subtitle=subtitle, test=test)
            # aria2 preallocates a sparse file and tracks completed ranges in
            # .aria2. Native HTTP resume requires a contiguous prefix; treating
            # that sparse file as downloaded bytes would silently corrupt audio.
            # Only discard this audio's unfinished aria2 file, never the video.
            partial = Path(name + '.part')
            control = Path(str(partial) + '.aria2')
            if control.exists():
                partial.unlink(missing_ok=True)
                control.unlink()
            # yt-dlp downloads selected streams serially. Scope these settings
            # to this audio call, including failure, so video/other sites still
            # use the user's downloader. Reuse yt-dlp's cookies, proxy, retry,
            # progress hooks, Content-Range validation and .part/resume logic.
            original_params = ydl.params
            ydl.params = dict(original_params, external_downloader={'default': 'native'},
                              http_chunk_size=2 * 1024 * 1024)
            try:
                return original_dl(name, stream, subtitle=subtitle, test=test)
            finally:
                ydl.params = original_params

        ydl.dl = download
        ydl._bqd_audio_installed = True
        return [], info
