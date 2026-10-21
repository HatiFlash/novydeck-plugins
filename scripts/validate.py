#!/usr/bin/env python3
"""Validate the NovyDeck community registry and optionally verify plugin archives."""
import argparse
import hashlib
import json
import re
import sys
import urllib.error
import urllib.request
import zipfile
from pathlib import Path
from urllib.parse import urlparse

MAX_ARCHIVE = 50 * 1024 * 1024
ID_RE = re.compile(r"^[a-z0-9][a-z0-9.-]*$")
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
VERSION_RE = re.compile(r"^[0-9]+(?:\.[0-9]+){0,7}$")


def fail(message):
    raise ValueError(message)


def https_url(value, label):
    if not isinstance(value, str):
        fail(f"{label}: doit être une URL HTTPS")
    parsed = urlparse(value)
    if parsed.scheme != "https" or not parsed.hostname or parsed.username is not None or parsed.password is not None:
        fail(f"{label}: URL HTTPS sans identifiants requise")
    return parsed


def numeric_version(value, label):
    if not isinstance(value, str) or len(value) > 64 or not VERSION_RE.fullmatch(value):
        fail(f"{label}: version invalide")
    return [int(part) for part in value.split(".")]


def validate_registry(path):
    try:
        raw = Path(path).read_bytes()
        registry = json.loads(raw)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        fail(f"Catalogue illisible ou JSON invalide: {exc}")
    if not isinstance(registry, dict) or set(registry) != {"schema", "plugins"}:
        fail("Le catalogue doit contenir uniquement schema et plugins")
    if type(registry["schema"]) is not int or registry["schema"] != 1:
        fail("Format du catalogue non pris en charge (schema doit valoir 1)")
    plugins = registry["plugins"]
    if not isinstance(plugins, list) or len(plugins) > 1000:
        fail("plugins doit être une liste de 1000 éléments maximum")
    seen = set()
    for i, plugin in enumerate(plugins):
        label = f"plugins[{i}]"
        if not isinstance(plugin, dict):
            fail(f"{label}: doit être un objet")
        required = {"id", "name", "author", "version", "description", "homepage", "download", "sha256", "license", "tested_decks"}
        allowed = required | {"description_fr", "min_novydeck"}
        if not required <= plugin.keys() or plugin.keys() - allowed:
            fail(f"{label}: champs requis manquants ou champs inconnus")
        ident = plugin["id"]
        if not isinstance(ident, str) or not ID_RE.fullmatch(ident) or ident in seen:
            fail(f"{label}: identifiant invalide ou en double")
        seen.add(ident)
        for field in ("name", "author", "description", "license"):
            value = plugin[field]
            if not isinstance(value, str) or not value.strip():
                fail(f"{label}.{field}: valeur non vide requise")
        numeric_version(plugin["version"], f"{label}.version")
        if "min_novydeck" in plugin:
            numeric_version(plugin["min_novydeck"], f"{label}.min_novydeck")
        if "description_fr" in plugin and not isinstance(plugin["description_fr"], str):
            fail(f"{label}.description_fr: doit être une chaîne")
        https_url(plugin["homepage"], f"{label}.homepage")
        download = https_url(plugin["download"], f"{label}.download")
        if not (download.path.endswith(".streamDeckPlugin") or download.path.endswith(".zip")):
            fail(f"{label}.download: extension .streamDeckPlugin ou .zip requise")
        if not isinstance(plugin["sha256"], str) or not SHA_RE.fullmatch(plugin["sha256"]):
            fail(f"{label}.sha256: SHA-256 minuscule de 64 caractères requise")
        decks = plugin["tested_decks"]
        if not isinstance(decks, list) or any(not isinstance(deck, str) for deck in decks):
            fail(f"{label}.tested_decks: doit être une liste de chaînes")
    return plugins


class LimitedRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urlparse(newurl).scheme != "https":
            fail("Redirection vers une URL non HTTPS refusée")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def check_archive(plugin, index):
    url = plugin["download"]
    request = urllib.request.Request(url, headers={"User-Agent": "NovyDeck-community-registry-validator/1.0"})
    opener = urllib.request.build_opener(LimitedRedirectHandler())
    try:
        with opener.open(request, timeout=30) as response:
            if urlparse(response.geturl()).scheme != "https":
                fail(f"plugins[{index}]: destination finale non HTTPS")
            declared = response.headers.get("Content-Length")
            if declared is not None and int(declared) > MAX_ARCHIVE:
                fail(f"plugins[{index}]: archive dépasse 50 Mo")
            chunks = bytearray()
            while True:
                block = response.read(min(65536, MAX_ARCHIVE + 1 - len(chunks)))
                if not block:
                    break
                chunks.extend(block)
                if len(chunks) > MAX_ARCHIVE:
                    fail(f"plugins[{index}]: archive dépasse 50 Mo")
    except (urllib.error.URLError, TimeoutError, OSError, ValueError) as exc:
        fail(f"plugins[{index}]: téléchargement impossible: {exc}")
    actual = hashlib.sha256(chunks).hexdigest()
    if actual != plugin["sha256"]:
        fail(f"plugins[{index}]: SHA-256 incorrect (attendu {plugin['sha256']}, obtenu {actual})")
    try:
        import io
        with zipfile.ZipFile(io.BytesIO(chunks)) as archive:
            infos = archive.infolist()
            if any(info.flag_bits & 1 for info in infos):
                fail(f"plugins[{index}]: archive chiffrée/protégée refusée")
            sdplugin_dirs = {part for info in infos for part in info.filename.split("/") if part.endswith(".sdPlugin")}
            if not sdplugin_dirs:
                fail(f"plugins[{index}]: aucun dossier .sdPlugin dans l'archive")
            for directory in sdplugin_dirs:
                manifest_name = directory.rstrip("/") + "/manifest.json"
                try:
                    manifest_info = archive.getinfo(manifest_name)
                except KeyError:
                    fail(f"plugins[{index}]: {directory} ne contient pas manifest.json")
                if manifest_info.flag_bits & 1:
                    fail(f"plugins[{index}]: manifest chiffré/protégé refusé")
                try:
                    json.loads(archive.read(manifest_name))
                except (UnicodeDecodeError, json.JSONDecodeError, RuntimeError) as exc:
                    fail(f"plugins[{index}]: manifest.json illisible: {exc}")
    except zipfile.BadZipFile as exc:
        fail(f"plugins[{index}]: archive ZIP invalide: {exc}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry", default="registry.json", help="chemin vers registry.json")
    parser.add_argument("--download", action="store_true", help="télécharger et inspecter les archives")
    args = parser.parse_args()
    try:
        plugins = validate_registry(args.registry)
        if args.download:
            for index, plugin in enumerate(plugins):
                check_archive(plugin, index)
        print(f"OK: registre valide ({len(plugins)} plugin(s))" + (", archives vérifiées" if args.download else ""))
        return 0
    except (ValueError, OSError) as exc:
        print(f"ERREUR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
