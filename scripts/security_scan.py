#!/usr/bin/env python3
"""Non-executing registry archive inspection on a Windows Defender runner."""
import argparse
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tempfile
from urllib.parse import urlparse
import zipfile

from validate import MAX_ARCHIVE, fetch_limited, validate_registry

SCRIPT_SUFFIXES = {'.js', '.ps1', '.bat', '.cmd', '.vbs'}
BINARY_SUFFIXES = {'.exe', '.dll', '.com', '.scr', '.msi'}
RULES = {
    'encoded_powershell': (r'powershell(?:\.exe)?\s+[^\r\n]{0,160}-(?:enc|encodedcommand)\b', 'high'),
    'defender_disable': (r'Set-MpPreference\s+[^\r\n]{0,160}DisableRealtimeMonitoring|sc\s+stop\s+WinDefend', 'high'),
    'run_key': (r'\\(?:CurrentVersion|Microsoft\\Windows)\\Run(?:Once)?\b', 'review'),
    'download_execute': (r'(?:Invoke-WebRequest|DownloadString|curl\s|wget\s)[^\r\n]{0,300}(?:Invoke-Expression|\biex\b|Start-Process|\|\s*(?:sh|bash))', 'high'),
    'obfuscated_eval': (r'\beval\s*\(\s*(?:atob|unescape|String\.fromCharCode)\s*\(', 'high'),
    'long_base64': (r'[A-Za-z0-9+/]{300,}={0,2}', 'review'),
}
URL_RE = re.compile(r'https?://[^\s\x00<>"\']+', re.I)
MAX_UNPACKED = 200 * 1024 * 1024


def finding(severity, rule, file, detail):
    return {'severity': severity, 'rule': rule, 'file': file, 'detail': detail[:320]}


def source_match(plugin):
    home = urlparse(plugin['homepage'])
    release = urlparse(plugin['download'])
    hp = home.path.strip('/').split('/')
    rp = release.path.strip('/').split('/')
    return (home.hostname or '').lower() == 'github.com' and (release.hostname or '').lower() == 'github.com' and len(hp) >= 2 and len(rp) >= 5 and [s.lower() for s in hp[:2]] == [s.lower() for s in rp[:2]] and rp[2:4] == ['releases', 'download']


def defender_scan(directory):
    candidates = [Path(os.environ.get('ProgramFiles', '')) / 'Windows Defender' / 'MpCmdRun.exe',
                  Path(os.environ.get('ProgramData', '')) / 'Microsoft' / 'Windows Defender' / 'Platform']
    cmd = candidates[0]
    if not cmd.is_file() and candidates[1].is_dir():
        versions = sorted(candidates[1].glob('*/MpCmdRun.exe'), reverse=True)
        if versions:
            cmd = versions[0]
    if not cmd.is_file():
        return 'error', 'MpCmdRun.exe unavailable'
    try:
        result = subprocess.run([str(cmd), '-Scan', '-ScanType', '3', '-File', str(directory), '-DisableRemediation'], capture_output=True, text=True, errors='replace', timeout=300)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return 'error', str(exc)
    return ({0: 'clean', 2: 'threat'}.get(result.returncode, 'error'),
            (result.stdout + '\n' + result.stderr)[-1200:])


def signature(path):
    # Pass a path as data, not interpolated into a PowerShell expression.
    script = 'Get-AuthenticodeSignature -LiteralPath $env:NOVYDECK_SCAN_FILE | Select-Object -ExpandProperty Status'
    try:
        result = subprocess.run(['powershell', '-NoProfile', '-NonInteractive', '-Command', script],
                                env={**os.environ, 'NOVYDECK_SCAN_FILE': str(path)}, capture_output=True,
                                text=True, timeout=30)
        return result.stdout.strip() if result.returncode == 0 else 'Error'
    except (OSError, subprocess.TimeoutExpired):
        return 'Error'


def inspect_archive(data, directory, plugin):
    results = []
    domains = set()
    manifests = []
    unpacked = 0
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        for info in archive.infolist():
            name = info.filename.replace('\\', '/')
            parts = PurePosixPath(name).parts
            if info.is_dir():
                continue
            if (info.flag_bits & 1 or info.file_size > MAX_ARCHIVE or not parts or
                    any(part in ('..', '.') or ':' in part for part in parts) or name.startswith('/') or
                    (info.external_attr >> 16) & 0o170000 == 0o120000):
                raise ValueError(f'Unsafe archive member: {name[:120]}')
            unpacked += info.file_size
            if unpacked > MAX_UNPACKED:
                raise ValueError('Unpacked archive exceeds 200 MiB')
            dest = directory.joinpath(*parts)
            dest.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(info) as source, dest.open('wb') as target:
                shutil.copyfileobj(source, target)
            suffix = dest.suffix.lower()
            if suffix in BINARY_SUFFIXES:
                results.append(finding('review', 'native_binary', name, f'Executable/DLL present; Authenticode: {signature(dest)}'))
            if suffix in SCRIPT_SUFFIXES:
                text = dest.read_bytes()[:2 * 1024 * 1024].decode('utf-8', errors='replace')
                for rule, (pattern, severity) in RULES.items():
                    if re.search(pattern, text, re.I):
                        results.append(finding(severity, rule, name, 'Static heuristic match; manual review required'))
                domains.update(m.group(0).rstrip('.,;)') for m in URL_RE.finditer(text))
            if dest.name.lower() == 'manifest.json' and '.sdPlugin' in name:
                manifests.append((name, json.loads(dest.read_text(encoding='utf-8'))))
    if not manifests:
        results.append(finding('high', 'missing_manifest', '', 'No .sdPlugin manifest.json'))
    for name, manifest in manifests:
        if not isinstance(manifest, dict):
            results.append(finding('high', 'invalid_manifest', name, 'Manifest is not an object'))
            continue
        codepath = manifest.get('CodePathWin', manifest.get('CodePath', ''))
        os_list = manifest.get('OS', [])
        if not isinstance(codepath, str) or not codepath:
            results.append(finding('review', 'manifest_codepath', name, 'No Windows CodePath declared'))
        elif not (directory / Path(name).parent / codepath).is_file():
            results.append(finding('high', 'manifest_codepath_missing', name, f'CodePath missing: {codepath[:120]}'))
        if manifest.get('Version') != plugin['version']:
            results.append(finding('review', 'manifest_version', name, f"Declared {manifest.get('Version')!s}; registry {plugin['version']}"))
        if not isinstance(os_list, list) or not any(isinstance(item, dict) and item.get('Platform', '').lower() == 'windows' for item in os_list):
            results.append(finding('review', 'manifest_os', name, 'Windows OS not declared'))
    if not any(p.is_file() and (p.name.lower().startswith('license') or p.name.lower().startswith('licence')) for p in directory.rglob('*')):
        results.append(finding('review', 'missing_license', '', 'No license file in the archive'))
    if not source_match(plugin):
        results.append(finding('review', 'source_mismatch', '', 'Release URL does not match homepage GitHub repository'))
    return results, sorted(domains), unpacked


def scan(plugin):
    result = {'id': plugin['id'], 'version': plugin['version'], 'sha256': plugin['sha256'], 'date': datetime.now(timezone.utc).isoformat(),
              'defender': 'error', 'findings': [], 'verdict': 'failed'}
    try:
        data = fetch_limited(plugin['download'], MAX_ARCHIVE, plugin['id'])
        if hashlib.sha256(data).hexdigest() != plugin['sha256']:
            result['findings'].append(finding('high', 'sha256_mismatch', '', 'Downloaded archive does not match registry SHA-256'))
            return result
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            findings, urls, size = inspect_archive(data, directory, plugin)
            result.update({'findings': findings, 'urls': urls, 'archive_bytes': len(data), 'unpacked_bytes': size})
            result['defender'], message = defender_scan(directory)
            if result['defender'] != 'clean':
                result['findings'].append(finding('high', 'defender_' + result['defender'], '', message))
    except (ValueError, OSError, zipfile.BadZipFile, json.JSONDecodeError, RuntimeError) as exc:
        result['findings'].append(finding('high', 'scan_error', '', str(exc)))
    result['verdict'] = ('failed' if result['defender'] == 'threat' or any(f['severity'] == 'high' for f in result['findings']) else
                         'review' if result['defender'] != 'clean' or result['findings'] else 'passed')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--registry', default='registry.json')
    parser.add_argument('--output', default='security/scan-results.json')
    args = parser.parse_args()
    plugins = validate_registry(args.registry)
    results = [scan(plugin) for plugin in plugins]
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({'plugins': results}, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    summary = ['## Security scan', '', '| Plugin | Defender | Verdict | Findings |', '| --- | --- | --- | --- |']
    for item in results:
        summary.append(f"| {item['id']} | {item['defender']} | {item['verdict']} | {', '.join(f['rule'] for f in item['findings']) or 'none'} |")
    text = '\n'.join(summary) + '\n'
    print(text)
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a', encoding='utf-8') as file:
            file.write(text)
    return int(any(item['verdict'] == 'failed' for item in results))


if __name__ == '__main__':
    sys.exit(main())
