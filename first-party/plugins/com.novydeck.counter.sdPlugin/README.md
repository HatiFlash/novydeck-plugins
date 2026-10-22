# NovyDeck Counter / Compteur

Native Windows plugin: counter (tap increments by the configured step, hold ≥ 600 ms resets; value persisted using Stream Deck settings), quick note (tap copies configured text), clipboard (hold saves current text; tap copies saved text). Clipboard operations use the Windows session clipboard; failures show an alert. Nothing is sent to a server. Text is stored in host action settings; avoid sensitive content.

Plugin Windows natif : compteur (clic ajoute le pas, appui ≥ 600 ms remet à zéro, valeur persistée dans les réglages), note rapide (clic copie le texte défini), presse-papiers (appui long mémorise le texte courant, clic recopie le texte mémorisé). En cas d'erreur du presse-papiers : alerte. Aucun envoi réseau. Le texte est stocké dans les réglages de l'action : éviter les secrets.

Install the `.streamDeckPlugin` archive produced by Windows CI. Source: `first-party/counter` and `first-party/novydeck-sdk`.
