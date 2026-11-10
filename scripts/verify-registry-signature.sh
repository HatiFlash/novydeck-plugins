#!/usr/bin/env bash
# Vérifie registry.json.sig (Ed25519, audit S-02) avec les deux clés publiques épinglées dans NovyDeck 0.19.3+.
# Message signé : "NovyDeck registry v1\n" suivi des octets exacts de registry.json.
set -euo pipefail
cd "$(dirname "$0")/.."
KEYS=(wDsBxy02b5CcI0CbNCQe0gHooUDH/j+K+TUIDqXSo4Q= VqvMDqAPWMpjaD0GeHSowJ7A6snekH4NQDvNwAFRjeg=)
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
[ -s registry.json.sig ] || { echo "registry.json.sig absent : le mainteneur doit signer le catalogue" >&2; exit 1; }
{ printf 'NovyDeck registry v1\n'; cat registry.json; } > "$T/msg"
tr -d ' \r\n\t' < registry.json.sig | base64 -d > "$T/sig" 2>/dev/null || { echo "registry.json.sig illisible" >&2; exit 1; }
[ "$(wc -c < "$T/sig")" -eq 64 ] || { echo "registry.json.sig : longueur incorrecte" >&2; exit 1; }
for k in "${KEYS[@]}"; do
  { printf '\x30\x2a\x30\x05\x06\x03\x2b\x65\x70\x03\x21\x00'; printf '%s' "$k" | base64 -d; } > "$T/pub.der"
  if openssl pkeyutl -verify -pubin -keyform DER -inkey "$T/pub.der" -rawin -in "$T/msg" -sigfile "$T/sig" >/dev/null 2>&1; then
    echo "registry.json.sig valide"; exit 0
  fi
done
echo "registry.json.sig ne correspond pas à registry.json : le catalogue doit être re-signé par le mainteneur" >&2
exit 1
