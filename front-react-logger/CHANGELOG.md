# Changelog du lecteur et compagnon

## Lecteur 0.5.1 / compagnon 0.3.1 — 24 septembre 2026

- Arbre et pagination triés par nom décroissant dans chaque dossier, pour les fichiers et les dossiers ; le contenu des journaux conserve son ordre de lecture.
- Test navigateur sur l’ordre affiché et test Rust sur les limites entre pages.

## Lecteur 0.5.0 — 24 septembre 2026

- Deux écrans Expo web copiables, `RloggerLogsScreen` et `ExternalLogsScreen`, regroupés avec leurs dépendances dans `src/log-viewer/`.
- Le lecteur autonome utilise ces mêmes écrans ; son menu et ses styles globaux restent séparés du module copiable.
- Styles du module limités à `.rlogger-viewer`, préférences de dossier propres à chaque source et session du compagnon fermée à la sortie de l’écran.
- [Guide d’intégration Expo](docs/INTEGRATION-EXPO.md) : copie du module, routes, liens dans un menu existant et build servi par le compagnon Rust sur la même origine.
- Vérifications locales : types, formatage, export Expo, six tests unitaires, 28 scénarios WebKit du lecteur autonome et un parcours WebKit dans une application hôte temporaire.

## Lecteur 0.4.0 — 23 septembre 2026

- Menu séparant les journaux RLOGGER privés et les journaux externes ; chaque entrée mémorise et rouvre son dossier.
- Maintenance automatique demandée uniquement pour le parcours RLOGGER, sans avertissement de maintenance pour les dossiers externes.
- `npm run dev` relance l’export Expo après modification et recharge la page via le compagnon Rust, sans serveur Node.

## Évolution locale — 23 septembre 2026

- Téléchargement et suppression manuelle de tous les fichiers réguliers, sans restriction de format ou d’heure.
- Actions sur les dossiers : archive TAR non compressée et suppression récursive confirmée.
- Toutes les extensions visibles ; fichiers et dossiers cachés (nom commençant par un point, dont `.rlogger`) masqués dans l’arbre, mais inclus dans les tailles et opérations récursives sur leur parent.
- Icônes de téléchargement et de suppression uniformes à droite de chaque fichier et dossier, y compris les fichiers non sélectionnés et les dossiers repliés ; gros boutons retirés du lecteur.
- Infobulles, libellés accessibles et confirmation conservés ; Annuler/Échap rend le focus à l’icône de suppression. Suivi fermé après suppression d’un parent.
- TypeScript, build Expo et 24 scénarios WebKit bureau/mobile validés après ces changements d’interface.
- Préconditions, confinement et verrous conservés, avec protection des dossiers ancêtres pendant les transferts.
- [Contrat et validation](../docs/11-actions-fichiers-dossiers.md).

## 0.3.0 — 2026-09-13

- Tailles récursives contenu/allocation, BigInt, état de calcul/partiel/indisponible.
- Gestion du stockage RLOGGER 2, historique générique conservé en consultation.
- Téléchargement HTTP par ticket et flux borné, suppression explicite sous précondition.
- Récupération des actifs abandonnés et nettoyage des anciennes journées vides.
- Suivi du même fichier après publication sans perdre sa fenêtre de lecture.
- Confirmation modale, motifs de protection et préservation de l’arbre après suppression.
- Backend et outillage Rust, Expo/gluestack et protocole de lecture v1 conservés.

Ruptures du logger 0.2.0 détaillées dans son changelog. Aucune publication externe.
