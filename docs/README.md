# Documentation de RLOGGER

Le dépôt contient la bibliothèque Rust `rlogger` 0.2.0, un compagnon HTTP/WebSocket
Rust et un lecteur local Expo/gluestack. Le [guide de lancement](../README.md)
explique comment les utiliser. Les données de mesure, captures et documents de
pilotage restent hors du dépôt public.

La bibliothèque permet de suivre une exécution à travers plusieurs threads,
tâches async et instances de logger, avec une écriture déportée et des fichiers
lisibles. Le lecteur ouvre un dossier local, parcourt son arborescence et suit
le fichier sélectionné. Cette démo suppose un compte machine fiable : une
application intégratrice doit protéger son propre point d’entrée et empêcher
l’accès direct au compagnon depuis des clients non fiables.

## Guides techniques

| Document | Contenu |
| --- | --- |
| [Vision et périmètre](01-vision-et-perimetre.md) | Usages et objectifs |
| [Fonctionnement et chronologie](02-fonctionnement-et-chronologie.md) | Capture, ordre et surcharge |
| [Instances, contexte et fichiers](03-instances-contexte-et-fichiers.md) | Contexte, routage et rotations |
| [Configuration et intégration](04-configuration-et-integration.md) | API, adaptateurs et cycle de vie |
| [Interface web locale](06-interface-web.md) | Parcours et rôle du service local |
| [Contrats d’implémentation](07-contrats-implementation.md) | Invariants du logger |
| [Format des journaux](08-format-v1.md) | Format et lecture |
| [Stockage horaire](09-migration-arborescence-rlogger.md) | Arborescence et migration |
| [Actions sur fichiers et dossiers](11-actions-fichiers-dossiers.md) | Téléchargement et suppression |
| [Compatibilité](COMPATIBILITE.md) | Versions et limites connues |
| [Lecteur local](../front-react-logger/docs/README.md) | Spécification et protocole |
| [Intégration Expo web](../front-react-logger/docs/INTEGRATION-EXPO.md) | Écrans copiables et liens dans un menu existant |

## Organisation

```text
lib-rust-logger/    Bibliothèque Rust et exemples d’usage
local-logs-server/  Compagnon local Rust
front-react-logger/ Lecteur Expo/gluestack
docs/               Guides techniques en français
```
