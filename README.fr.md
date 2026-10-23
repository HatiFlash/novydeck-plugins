# Plugins communautaires NovyDeck

[English](README.md) | [Français](README.fr.md)

Un catalogue de plugins libres pour [NovyDeck](https://github.com/HatiFlash/NovyDeck). Consultez [CONTRIBUTING.fr.md](CONTRIBUTING.fr.md) pour proposer un plugin.

## Plugins NovyDeck officiels

Sources et empaquetage : [first-party/](first-party/). Les sept plugins natifs sont Horloge, Compteur, Moniteur système, Lecture en cours, Économiseur OLED, Home Assistant et Commande Google Assistant. Home Assistant exige un serveur et un jeton personnels ; Google Assistant exige un projet Google Cloud personnel et OAuth bureau. Ces deux intégrations n'ont pas été testées avec un vrai serveur/compte.

## Plugins communautaires

Le catalogue est en préparation ; seules les archives officielles aux empreintes vérifiées seront publiées.

Chaque fiche inclut page et release officielles HTTPS, SHA-256, licence libre, version exacte, decks testés (liste vide si non testés) et `min_novydeck` obligatoire. Champs facultatifs : `description_fr`, `uuid`, `category` (`system`, `audio`, `streaming`, `productivity`, `smart-home`, `communication`, `gaming`, `utilities`), `summary`/`summary_fr` (140 caractères maximum), `icon` HTTPS PNG carré (256 Kio maximum), six `screenshots` HTTPS PNG/JPEG maximum (2 Mio et 4096 pixels par côté), 100 `actions` maximum (`name`, `name_fr` facultatif, 120 octets UTF-8), huit `tags` maximum (40 caractères) et `requires` (une chaîne de 200 caractères maximum). Voir l'[exemple](scripts/example-entry.json).

## Badges de sécurité

Le magasin de NovyDeck affiche jusqu'à trois badges par plugin :

- **Officiel** : publié par NovyDeck (`com.novydeck.*`) depuis les releases de ce dépôt.
- **Analysé** : l'archive exacte du catalogue (même SHA-256 et même version) a passé le contrôle automatique. Un workflow GitHub Actions télécharge chaque archive, vérifie son SHA-256, l'analyse avec Microsoft Defender et recherche les contenus à risque (exécutables et leurs signatures, scripts obfusqués, téléchargement puis exécution, clés de démarrage, cohérence du manifest, licence et source officielle). Il tourne à chaque modification du catalogue, chaque semaine et à la demande, sans intervention manuelle ; les résultats sont publiés dans [`security/scan-results.json`](security/scan-results.json).
- **Testé par NovyDeck** : cette version précise a été essayée à la main dans NovyDeck (champ `tested_by_novydeck`).

Un plugin dont l'analyse est à examiner affiche un avertissement dans NovyDeck. Une analyse réduit les risques sans les supprimer, et les plugins s'exécutent avec vos droits utilisateur.

## Utiliser la bibliothèque communautaire dans NovyDeck

Le catalogue prévoit trois badges distincts : « Officiel » (`com.novydeck.*`, origine), « Analysé » (résultat automatique réussi pour le même SHA-256 dans `security/scan-results.json`) et « Testé par NovyDeck » (test manuel de la version exacte, champ `tested_by_novydeck` avec date/version). GitHub Actions vérifie l’empreinte, Microsoft Defender et des heuristiques statiques sur les PR et chaque semaine. Une analyse ne garantit pas l’absence de risque ; les trouvailles « review » nécessitent un examen humain.

Dans l’application, ouvrez **Bibliothèque → Plugins → Communauté**. La connexion à GitHub nécessite votre accord ; après l’avoir donné, confirmez la connexion. NovyDeck vérifie le SHA-256 publié de l’archive téléchargée avant son importation. Les plugins communautaires sans badge Officiel portent la mention **« Communauté — non vérifié »** : une empreinte correspondante confirme le téléchargement, pas la sûreté du code tiers. Examinez chaque plugin avant de l’installer.

Consultez les [règles de contribution](CONTRIBUTING.fr.md) et le [schéma du registre](schema/registry.schema.json).
