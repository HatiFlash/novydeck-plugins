#!/usr/bin/env python3
"""Capture local property inspectors with Edge on Windows CI (no network requests)."""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
edge = Path(os.environ.get('PROGRAMFILES(X86)', r'C:\Program Files (x86)')) / 'Microsoft/Edge/Application/msedge.exe'
if not edge.is_file():
    raise SystemExit(f'Edge not installed: {edge}')
output = ROOT / 'inspector-previews'
output.mkdir(exist_ok=True)
# The real inspector derives visible fields from actionInfo, not the initial HTML.
# Stub the local WebSocket only for these offline previews; no service is contacted.
FAKE_SOCKET = '''
class PreviewWebSocket {
    static OPEN = 1;
    constructor(url) { this.url = url; this.readyState = PreviewWebSocket.OPEN;
        setTimeout(() => this.onopen?.(), 0); }
    send(_message) {}
    close() { this.readyState = 3; }
}
window.WebSocket = PreviewWebSocket;
'''
for name in (('googlehome',) if os.environ.get('GOOGLEHOME_CAPTURE_ONLY') else ('clock', 'counter', 'oledsaver', 'homeassistant', 'googleassistant', 'googlehome', 'wallpaperengine')):
    html = (ROOT / 'plugins' / f'com.novydeck.{name}.sdPlugin' / 'inspector.html').read_text(encoding='utf-8')
    css = (ROOT / 'plugins' / f'com.novydeck.{name}.sdPlugin' / 'inspector.css').read_text(encoding='utf-8')
    actions = ('wallpaper', 'playlist', 'next', 'profile', 'pause', 'stopplay') if name == 'wallpaperengine' else (None,)
    for language in ('en', 'fr'):
        for action in actions:
            # Self-contained page: packaged CSS and genuine inspector initialization.
            page = html.replace('<link rel="stylesheet" href="inspector.css">', f'<style>{css}</style>')
            if name in ('wallpaperengine', 'googlehome'):
                info = json.dumps(json.dumps({'application': {'language': language}}))
                action_info = json.dumps(json.dumps({
                    'context': 'preview',
                    'action': (f'com.novydeck.wallpaperengine.{action}' if name == 'wallpaperengine'
                               else 'com.novydeck.googlehome.control'),
                    'payload': {'settings': ({'scope': 'group', 'home': 'sample-home',
                                             'targets': ['sample-lamp', 'sample-fan'], 'mode': 'toggle'}
                                            if name == 'googlehome' else {})}}))
                init = (FAKE_SOCKET + '\nconnectElgatoStreamDeckSocket('
                        f"0, 'preview', 'registerPropertyInspector', {info}, {action_info});")
                if name == 'googlehome':
                    fixture = {'context': 'preview', 'event': 'sendToPropertyInspector',
                               'payload': {'type': 'homes', 'homes': [
                                   {'id': 'sample-home', 'name': 'Demo (offline)',
                                    'rooms': [{'id': 'sample-room', 'name': 'Living room / Salon'}],
                                    'devices': [{'id': 'sample-lamp', 'name': 'Lamp / Lampe'},
                                                {'id': 'sample-fan', 'name': 'Fan / Ventilateur'}]}]}}
                    init += ("\nsetTimeout(() => ws.onmessage?.({data: JSON.stringify("
                             + json.dumps(fixture) + ")}), 20);")
                page = page.replace('</script></html>', f'</script><script>{init}</script></html>')
            elif language == 'fr':
                page = page.replace('translate();\n</script>', "lang='fr';translate();\n</script>")
                page = page.replace('translate();visible();\n</script>', "lang='fr';translate();visible();\n</script>")
            suffix = f'{name}-{action}-{language}' if action else f'{name}-{language}'
            target = output / f'{suffix}.html'
            target.write_text(page, encoding='utf-8')
            screenshot = output / f'{suffix}.png'
            try:
                subprocess.run([str(edge), '--headless=new', '--disable-gpu', '--hide-scrollbars',
                                '--no-first-run', f'--window-size=520,{1400 if name == "googlehome" else 1000}', f'--screenshot={screenshot}',
                                target.as_uri()], check=True, timeout=45)
                if not screenshot.is_file():
                    raise RuntimeError(f'Missing screenshot: {screenshot}')
            finally:
                target.unlink()
            print(screenshot)
