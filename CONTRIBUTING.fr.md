# Contribuer avec un plugin

Le catalogue communautaire est maintenu par HatiFlash pour NovyDeck. Les contributions sont les bienvenues, mais chaque proposition est examinée et peut être refusée ou retirée par le mainteneur.

## Proposer un plugin

1. Forkez ce dépôt.
2. Publiez l’archive du plugin (`.streamDeckPlugin`, ou un `.zip` contenant un dossier `.sdPlugin` avec un `manifest.json` lisible) à une URL HTTPS stable.
3. Calculez le SHA-256 de l’archive : sous Windows, exécutez `certutil -hashfile chemin-du-fichier SHA256` ; sous Linux/macOS, `sha256sum chemin-du-fichier`.
4. Ajoutez une entrée à `registry.json` en suivant `schema/registry.schema.json`. Indiquez la licence, les decks testés, une version exacte, les URL HTTPS de la page et du téléchargement, ainsi que le SHA-256 en 64 caractères minuscules.
5. Ouvrez une pull request et cochez sa liste de contrôle. La CI valide le catalogue, télécharge chaque archive, vérifie son empreinte et inspecte son contenu ZIP.

Ne proposez que des plugins que vous avez le droit de redistribuer. Déclarez les comportements et dépendances pertinents. Tout comportement malveillant, toute collecte cachée de données et toute métadonnée trompeuse sont interdits. Le mainteneur modère les propositions et n’est pas tenu d’accepter ou de conserver une entrée.

Un plugin répertorié est un code tiers qui n’a pas fait l’objet d’un audit ou d’une vérification de sûreté indépendante. Son installation se fait aux risques de l’utilisateur ; la vérification de l’empreinte confirme que les octets téléchargés correspondent au catalogue, pas que le code est fiable.
