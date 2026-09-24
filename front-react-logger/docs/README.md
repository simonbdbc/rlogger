# Documentation du lecteur local

Lecteur 0.4.0 / compagnon 0.3.0 : consultation générique, actions explicites
sur les fichiers et dossiers, et maintenance du stockage horaire privé.
[Guide utilisateur](../README.md), [contrat actuel des actions](../../docs/11-actions-fichiers-dossiers.md).

Le menu du lecteur Expo/gluestack sépare les racines privées RLOGGER, avec
maintenance automatique sous conditions, des dossiers externes, sans maintenance
ni avertissement RLOGGER. Chaque parcours conserve son propre chemin.
Un dossier mémorisé s’ouvre automatiquement quand son parcours est sélectionné.
Le lecteur ouvre un dossier local à partir de son chemin absolu,
affiche son arborescence à gauche et suit le contenu du fichier cliqué à droite.
Il est accompagné d’un service local de lecture ; cette démo ne prévoit ni compte
ni authentification utilisateur. Le jeton de session opaque ne prouve pas
l’identité. Une application intégratrice doit protéger son propre point d’entrée
et empêcher l’accès direct au compagnon depuis des clients non fiables.

## Documents

| Document | Rôle |
| --- | --- |
| [Spécification](SPECIFICATION.md) | Parcours, disposition, états, suivi et limites |
| [Architecture locale](ARCHITECTURE-LOCALE.md) | Compagnon, API, positions de lecture et ressources |
| [Documentation du projet](../../docs/README.md) | Bibliothèque, stockage et contrat des actions |

## Périmètre et statut

Expo, gluestack, Rust (Axum/Tokio), HTTP/WS v1 et les budgets sont livrés.
La lecture brute est indépendante de la crate. FRONT-028–030 restent différés.
Les versions et plateformes réellement testées figurent dans le bilan de compatibilité.

Le chantier frontend possède le compagnon local et son packaging. Ce composant
n’est ni ajouté à la crate Rust ni lancé par un appel au logger. Le lecteur brut
peut être livré sur fichiers génériques sans dépendre de l’adaptateur tracing,
d’un journal JSON ou d’une interface structurée avancée.

## Organisation livrée

- `app/` : routes Expo Router et cycle de vie React.
- `src/` : arbre, lecteur virtualisé, gluestack, préférences, client HTTP/WS.
- `shared/` : protocole et fenêtre d’octets.
- `../local-logs-server/` : compagnon Rust, confinement, lecture et tests.
- `e2e/` : scénarios WebKit desktop et petite largeur.
- `../local-logs-server/examples/fixtures.rs` : génération Rust des fichiers synthétiques de démonstration.
- `dist/` : build Expo généré, servi par npm start.
- `.local-logs-dev/` : exports temporaires du mode `npm run dev`, ignorés par Git.
