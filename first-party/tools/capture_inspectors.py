#!/usr/bin/env python3
"""Capture local property inspectors with Edge on Windows CI (no network requests)."""
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
edge = Path(os.environ.get('PROGRAMFILES(X86)', r'C:\Program Files (x86)')) / 'Microsoft/Edge/Application/msedge.exe'
if not edge.is_file():
    raise SystemExit(f'Edge not installed: {edge}')
output = ROOT / 'inspector-previews'
output.mkdir(exist_ok=True)
for name in ('clock', 'counter', 'oledsaver', 'homeassistant', 'googleassistant', 'googlehome', 'wallpaperengine'):
    html = (ROOT / 'plugins' / f'com.novydeck.{name}.sdPlugin' / 'inspector.html').read_text(encoding='utf-8')
    css = (ROOT / 'plugins' / f'com.novydeck.{name}.sdPlugin' / 'inspector.css').read_text(encoding='utf-8')
    for language in ('en', 'fr'):
        # Self-contained temporary page so the preview uses exactly the packaged CSS.
        page = html.replace('<link rel="stylesheet" href="inspector.css">', f'<style>{css}</style>')
        if language == 'fr':
            page = page.replace('translate();\n</script>', "lang='fr';translate();\n</script>")
            page = page.replace('translate();visible();\n</script>', "lang='fr';translate();visible();\n</script>")
        target = output / f'{name}-{language}.html'
        target.write_text(page, encoding='utf-8')
        screenshot = output / f'{name}-{language}.png'
        subprocess.run([str(edge), '--headless=new', '--disable-gpu', '--hide-scrollbars',
                        '--no-first-run', '--window-size=520,560', f'--screenshot={screenshot}',
                        target.as_uri()], check=True, timeout=45)
        if not screenshot.is_file():
            raise RuntimeError(f'Missing screenshot: {screenshot}')
        target.unlink()
        print(screenshot)
