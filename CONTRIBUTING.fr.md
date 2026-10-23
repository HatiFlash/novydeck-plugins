# Contribuer avec un plugin

Le catalogue communautaire est maintenu par HatiFlash pour NovyDeck. Les contributions sont les bienvenues, mais chaque proposition est examinée et peut être refusée ou retirée par le mainteneur.

## Proposer un plugin

1. Forkez ce dépôt.
2. Publiez votre plugin libre sous une licence libre vérifiable, accompagné d'une release officielle (`.streamDeckPlugin`, ou un `.zip` contenant un dossier `.sdPlugin` avec un `manifest.json` lisible) à une URL HTTPS stable. Testez-le avant de le proposer.
3. Calculez le SHA-256 de l’archive : sous Windows, exécutez `certutil -hashfile chemin-du-fichier SHA256` ; sous Linux/macOS, `sha256sum chemin-du-fichier`.
4. Ajoutez une entrée à `registry.json` en suivant [le schéma](schema/registry.schema.json) et [l'exemple](scripts/example-entry.json). Indiquez la licence, les decks réellement testés (ne revendiquez pas une version non testée), la version exacte, les URL HTTPS de la page et de la release, le SHA-256 en 64 caractères minuscules et `min_novydeck` obligatoire. Champs facultatifs : `description_fr`, `uuid` (minuscules, chiffres, points, tirets), `category` (`system`, `audio`, `streaming`, `productivity`, `smart-home`, `communication`, `gaming`, `utilities`), `summary`/`summary_fr` (140 caractères), `icon` PNG carré (256 Kio), jusqu'à six `screenshots` PNG/JPEG (2 Mio, 4096 pixels), 100 `actions` (nom et nom français facultatif, 120 octets UTF-8), huit `tags` (40 caractères) et `requires` (une chaîne de 200 caractères). Les URL des images doivent utiliser HTTPS.
5. Ouvrez une pull request et cochez sa liste de contrôle. La CI valide le catalogue, télécharge chaque archive, vérifie son empreinte et inspecte son contenu ZIP.

Ne proposez que des plugins que vous avez le droit de redistribuer. Déclarez les comportements et dépendances pertinents. Tout comportement malveillant, toute collecte cachée de données et toute métadonnée trompeuse sont interdits. Le mainteneur modère les propositions et n’est pas tenu d’accepter ou de conserver une entrée.

Badges : « Officiel » pour `com.novydeck.*` ; « Analysé » uniquement si l’archive correspond au SHA-256 et si Defender est propre et l’analyse statique sans trouvaille sévère (`security/scan-results.json`, même version/empreinte) ; « Testé par NovyDeck » est une validation manuelle distincte, consignée par les mainteneurs via `tested_by_novydeck: {"version": "1.0.2", "date": "AAAA-MM-JJ"}` pour la version publiée exacte. Ne revendiquez pas vous-même ce dernier badge. GitHub Actions analyse automatiquement les archives à chaque PR, sur les pushes éligibles, chaque semaine et à la demande : signatures des binaires, scripts, URL, manifeste, origine et licence. Les résultats « review » demandent un examen humain.

Un plugin répertorié est un code tiers sans audit indépendant. Une analyse propre ne garantit pas l’absence de risque ; la vérification de l’empreinte confirme seulement les octets téléchargés. L’installation se fait aux risques de l’utilisateur.
