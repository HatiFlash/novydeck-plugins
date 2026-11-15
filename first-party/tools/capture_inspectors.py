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
for name in (('googlehome',) if os.environ.get('GOOGLEHOME_CAPTURE_ONLY') else ('clock', 'counter', 'oledsaver', 'rtssosd', 'homeassistant', 'googleassistant', 'googlehome', 'wallpaperengine')):
    html = (ROOT / 'plugins' / f'com.novydeck.{name}.sdPlugin' / 'inspector.html').read_text(encoding='utf-8')
    css = (ROOT / 'plugins' / f'com.novydeck.{name}.sdPlugin' / 'inspector.css').read_text(encoding='utf-8')
    actions = (('wallpaper', 'playlist', 'next', 'profile', 'pause', 'stopplay') if name == 'wallpaperengine'
               else ('toggle', 'service', 'state') if name == 'homeassistant' else (None,))
    # Scripts shipped next to the inspector (Home Assistant presets.js) must be inlined: the
    # temporary page lives in inspector-previews/, not in the plugin folder.
    presets = ROOT / 'plugins' / f'com.novydeck.{name}.sdPlugin' / 'presets.js'
    if presets.is_file():
        html = html.replace('<script src="presets.js"></script>', f'<script>{presets.read_text(encoding="utf-8")}</script>')
    for language in ('en', 'fr'):
        for action in actions:
            # Self-contained page: packaged CSS and genuine inspector initialization.
            page = html.replace('<link rel="stylesheet" href="inspector.css">', f'<style>{css}</style>')
            if name in ('wallpaperengine', 'googlehome', 'oledsaver', 'rtssosd', 'homeassistant'):
                info = json.dumps(json.dumps({'application': {'language': language}}))
                action_info = json.dumps(json.dumps({
                    'context': 'preview',
                    'action': (f'com.novydeck.wallpaperengine.{action}' if name == 'wallpaperengine'
                               else 'com.novydeck.oledsaver.saver' if name == 'oledsaver'
                               else 'com.novydeck.rtssosd.osd' if name == 'rtssosd'
                               else f'com.novydeck.homeassistant.{action}' if name == 'homeassistant'
                               else 'com.novydeck.googlehome.control'),
                    'payload': {'settings': ({'scope': 'group', 'home': 'sample-home',
                                             'targets': ['sample-lamp', 'sample-fan'], 'mode': 'toggle'}
                                            if name == 'googlehome' else
                                            {'mode': 'on', 'monitors': [{'name': r'\\.\DISPLAY2', 'index': 1,
                                             'x': 2560, 'y': 0, 'w': 1920, 'h': 1080}]}
                                            if name == 'oledsaver' else
                                            {'entity': 'light.neon', 'refresh': 10}
                                            if name == 'homeassistant' else {})}}))
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
                if name == 'homeassistant':
                    # Offline fake connection: the Toggle action must show its "Action" menu
                    # (no Domain/Service/JSON fields) for light.neon.
                    ha_context = {'context': 'preview', 'event': 'sendToPropertyInspector'}
                    for payload in (
                        {'type': 'status', 'connected': True, 'hasToken': True, 'code': 'connected',
                         'url': 'http://homeassistant.local:8123'},
                        {'type': 'list', 'status': 'ok', 'entities': [
                            {'id': 'light.neon', 'name': 'Neon / Néon', 'domain': 'light', 'state': 'on', 'unit': ''},
                            {'id': 'switch.plug', 'name': 'Plug / Prise', 'domain': 'switch', 'state': 'off', 'unit': ''}]},
                        {'type': 'groups', 'groups': []},
                    ):
                        init += ("\nsetTimeout(() => ws.onmessage?.({data: JSON.stringify("
                                 + json.dumps({**ha_context, 'payload': payload}) + ")}), 20);")
                if name == 'oledsaver':
                    fixture = {'context': 'preview', 'event': 'sendToPropertyInspector', 'payload': {'monitors': [
                        {'index': 0, 'name': r'\\.\DISPLAY1', 'width': 2560, 'height': 1440, 'x': 0, 'y': 0, 'primary': True},
                        {'index': 1, 'name': r'\\.\DISPLAY2', 'width': 1920, 'height': 1080, 'x': 2560, 'y': 0, 'primary': False},
                        {'index': 2, 'name': r'\\.\DISPLAY3', 'width': 1920, 'height': 1080, 'x': -1920, 'y': 0, 'primary': False}]}}
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
                                '--no-first-run', f'--window-size=520,{1400 if name == "googlehome" else 2000 if name == "homeassistant" else 1000}', f'--screenshot={screenshot}',
                                target.as_uri()], check=True, timeout=45)
                if not screenshot.is_file():
                    raise RuntimeError(f'Missing screenshot: {screenshot}')
            finally:
                target.unlink()
            print(screenshot)
