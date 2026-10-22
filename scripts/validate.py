#!/usr/bin/env python3
"""Validate the NovyDeck community registry and optionally verify plugin archives."""
import argparse
import hashlib
import json
import re
import struct
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
CATEGORIES = {"system", "audio", "streaming", "productivity", "smart-home", "communication", "gaming", "utilities"}
IMAGE_EXTENSIONS = {".png", ".jpg", ".jpeg"}


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
        required = {"id", "name", "author", "version", "description", "homepage", "download", "sha256", "license", "tested_decks", "min_novydeck"}
        allowed = required | {"description_fr", "uuid", "category", "summary", "summary_fr", "icon", "screenshots", "actions", "tags", "requires"}
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
        if "uuid" in plugin and (not isinstance(plugin["uuid"], str) or not re.fullmatch(r"[a-z0-9.-]+", plugin["uuid"])):
            fail(f"{label}.uuid: identifiant invalide")
        if "category" in plugin and (not isinstance(plugin["category"], str) or plugin["category"] not in CATEGORIES):
            fail(f"{label}.category: catégorie inconnue")
        for field, max_chars in (("summary", 140), ("summary_fr", 140), ("requires", 200)):
            if field in plugin and (not isinstance(plugin[field], str) or len(plugin[field]) > max_chars):
                fail(f"{label}.{field}: chaîne trop longue ou invalide")
        if "icon" in plugin:
            if Path(https_url(plugin["icon"], f"{label}.icon").path).suffix.lower() != ".png":
                fail(f"{label}.icon: URL PNG requise")
        for field, maximum in (("screenshots", 6), ("actions", 100), ("tags", 8)):
            if field in plugin and (not isinstance(plugin[field], list) or len(plugin[field]) > maximum):
                fail(f"{label}.{field}: liste trop longue ou invalide")
        for url in plugin.get("screenshots", []):
            if Path(https_url(url, f"{label}.screenshots").path).suffix.lower() not in IMAGE_EXTENSIONS:
                fail(f"{label}.screenshots: URL PNG/JPEG requise")
        for action in plugin.get("actions", []):
            if not isinstance(action, dict) or not {"name"} <= action.keys() or action.keys() - {"name", "name_fr"}:
                fail(f"{label}.actions: objet d'action invalide")
            if not isinstance(action["name"], str) or not action["name"].strip() or len(action["name"].encode()) > 120:
                fail(f"{label}.actions.name: nom invalide")
            if "name_fr" in action and (not isinstance(action["name_fr"], str) or len(action["name_fr"].encode()) > 120):
                fail(f"{label}.actions.name_fr: nom invalide")
        if any(not isinstance(tag, str) or not tag.strip() or len(tag) > 40 for tag in plugin.get("tags", [])):
            fail(f"{label}.tags: étiquette invalide")
    return plugins


class LimitedRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urlparse(newurl).scheme != "https":
            fail("Redirection vers une URL non HTTPS refusée")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch_limited(url, limit, label):
    request = urllib.request.Request(url, headers={"User-Agent": "NovyDeck-community-registry-validator/1.0"})
    opener = urllib.request.build_opener(LimitedRedirectHandler())
    try:
        with opener.open(request, timeout=30) as response:
            if urlparse(response.geturl()).scheme != "https":
                fail(f"{label}: destination finale non HTTPS")
            declared = response.headers.get("Content-Length")
            if declared is not None and int(declared) > limit:
                fail(f"{label}: fichier trop volumineux")
            data = response.read(limit + 1)
            if len(data) > limit:
                fail(f"{label}: fichier trop volumineux")
            return data
    except (urllib.error.URLError, TimeoutError, OSError, ValueError) as exc:
        fail(f"{label}: téléchargement impossible: {exc}")


def image_dimensions(data, icon, label):
    width = height = 0
    if data.startswith(b"\x89PNG\r\n\x1a\n") and len(data) >= 24 and data[12:16] == b"IHDR":
        width, height = struct.unpack(">II", data[16:24])
    elif not icon and data.startswith(b"\xff\xd8"):
        pos = 2
        width = height = 0
        while pos + 4 <= len(data):
            if data[pos] != 0xff:
                break
            marker = data[pos + 1]
            pos += 2
            if marker in (0xd8, 0xd9) or 0xd0 <= marker <= 0xd7:
                continue
            length = int.from_bytes(data[pos:pos + 2], "big")
            if length < 2 or pos + length > len(data):
                break
            if marker in (0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf) and length >= 7:
                height = int.from_bytes(data[pos + 3:pos + 5], "big")
                width = int.from_bytes(data[pos + 5:pos + 7], "big")
                break
            pos += length
    else:
        fail(f"{label}: format d'image invalide")
    if not (0 < width <= 4096 and 0 < height <= 4096) or icon and width != height:
        fail(f"{label}: dimensions invalides")


def check_images(plugin, index):
    if "icon" in plugin:
        label = f"plugins[{index}].icon"
        image_dimensions(fetch_limited(plugin["icon"], 256 * 1024, label), True, label)
    for i, url in enumerate(plugin.get("screenshots", [])):
        label = f"plugins[{index}].screenshots[{i}]"
        image_dimensions(fetch_limited(url, 2 * 1024 * 1024, label), False, label)


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
            # Comme NovyDeck, accepter les anciennes archives Windows dont les chemins utilisent « \\ ».
            by_name = {info.filename.replace("\\", "/"): info for info in infos}
            sdplugin_dirs = set()
            for name in by_name:
                parts = name.split("/")
                for i, part in enumerate(parts):
                    if part.endswith(".sdPlugin"):
                        sdplugin_dirs.add("/".join(parts[: i + 1]))
                        break
            if not sdplugin_dirs:
                fail(f"plugins[{index}]: aucun dossier .sdPlugin dans l'archive")
            for directory in sdplugin_dirs:
                manifest_name = directory.rstrip("/") + "/manifest.json"
                manifest_info = by_name.get(manifest_name)
                if manifest_info is None:
                    fail(f"plugins[{index}]: {directory} ne contient pas manifest.json")
                if manifest_info.flag_bits & 1:
                    fail(f"plugins[{index}]: manifest chiffré/protégé refusé")
                try:
                    json.loads(archive.read(manifest_info))
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
                check_images(plugin, index)
        print(f"OK: registre valide ({len(plugins)} plugin(s))" + (", archives vérifiées" if args.download else ""))
        return 0
    except (ValueError, OSError) as exc:
        print(f"ERREUR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
