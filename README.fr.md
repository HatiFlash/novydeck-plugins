# Plugins communautaires NovyDeck

[English](README.md) | [Français](README.fr.md)

Un catalogue de plugins libres pour [NovyDeck](https://github.com/HatiFlash/NovyDeck). Consultez [CONTRIBUTING.fr.md](CONTRIBUTING.fr.md) pour proposer un plugin.

## Plugins NovyDeck officiels

Sources et empaquetage : [first-party/](first-party/). Les sept plugins natifs sont Horloge, Compteur, Moniteur système, Lecture en cours, Économiseur OLED, Home Assistant et Commande Google Assistant. Home Assistant exige un serveur et un jeton personnels ; Google Assistant exige un projet Google Cloud personnel et OAuth bureau. Ces deux intégrations n'ont pas été testées avec un vrai serveur/compte.

## Plugins communautaires

Le catalogue est en préparation ; seules les archives officielles aux empreintes vérifiées seront publiées.

Chaque fiche inclut page et release officielles HTTPS, SHA-256, licence libre, version exacte, decks testés (liste vide si non testés) et `min_novydeck` obligatoire. Champs facultatifs : `description_fr`, `uuid`, `category` (`system`, `audio`, `streaming`, `productivity`, `smart-home`, `communication`, `gaming`, `utilities`), `summary`/`summary_fr` (140 caractères maximum), `icon` HTTPS PNG carré (256 Kio maximum), six `screenshots` HTTPS PNG/JPEG maximum (2 Mio et 4096 pixels par côté), 100 `actions` maximum (`name`, `name_fr` facultatif, 120 octets UTF-8), huit `tags` maximum (40 caractères) et `requires` (une chaîne de 200 caractères maximum). Voir l'[exemple](scripts/example-entry.json).

## Utiliser la bibliothèque communautaire dans NovyDeck

Le catalogue prévoit trois badges distincts : « Officiel » (`com.novydeck.*`, origine), « Analysé » (résultat automatique réussi pour le même SHA-256 dans `security/scan-results.json`) et « Testé par NovyDeck » (test manuel de la version exacte, champ `tested_by_novydeck` avec date/version). GitHub Actions vérifie l’empreinte, Microsoft Defender et des heuristiques statiques sur les PR et chaque semaine. Une analyse ne garantit pas l’absence de risque ; les trouvailles « review » nécessitent un examen humain. L’app affiche actuellement « Communauté — non vérifié » : ces badges de métadonnées n’y sont pas encore affichés automatiquement.

Dans l’application, ouvrez **Bibliothèque → Plugins → Communauté**. La connexion à GitHub nécessite votre accord ; après l’avoir donné, confirmez la connexion. NovyDeck vérifie le SHA-256 publié de l’archive téléchargée avant son importation. Les entrées portent la mention **« Communauté — non vérifié »** : une empreinte correspondante confirme le téléchargement, pas la sûreté du code tiers. Examinez chaque plugin avant de l’installer.

Consultez les [règles de contribution](CONTRIBUTING.fr.md) et le [schéma du registre](schema/registry.schema.json).
