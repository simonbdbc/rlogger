# Compatibilité — logger 0.2.0 / compagnon 0.3.2 / lecteur 0.5.2

> Extension locale du 23 septembre : les actions manuelles couvrent désormais tous
> les fichiers réguliers et dossiers dans la racine ouverte, avec téléchargement TAR
> et suppression récursive pour les dossiers. Le [nouveau contrat](11-actions-fichiers-dossiers.md)
> remplace les restrictions historiques du contrat horaire.

Livraison vérifiée localement sur macOS Apple Silicon. Les tests du projet et la
[CI](../.github/workflows/ci.yml) couvrent séparément la bibliothèque, le
compagnon et le lecteur. Le [contrat des actions](11-actions-fichiers-dossiers.md)
décrit les opérations manuelles actuelles.

| Élément | Version / contrat |
| --- | --- |
| Logger | RLOGGER, package et crate `rlogger` 0.2.0, MIT |
| Rust / MSRV | rustc/cargo 1.95.0, édition 2024 ; features optionnelles `log` et `tracing` |
| Stockage | Marqueur RLOGGER storage 2, rotation horaire obligatoire, segments actifs/finalisés/récupérés |
| Texte | RLOG/1 inchangé, UTF-8 avec contrôles échappés |
| Compagnon / frontend | local-logs-server 0.3.2, local-logs-viewer 0.5.2 |
| Protocole | HTTP /api/v1, WS local-logs.v1 ; extension additive des DTO et routes de gestion |
| Backend | Rust, Axum 0.8.9 et Tokio ; versions exactes verrouillées dans Cargo.lock |
| Outillage frontend | Node 24.18.0, npm 11.16.0 ; aucun serveur Node |
| Frontend | Expo 57.0.20, Router 57.0.19, React 19.2.3, gluestack core 5.0.15 |
| Environnement local vérifié | macOS arm64 |
| Navigateur exécuté | Playwright WebKit, 1280×800 et 390×844 |

## Ruptures intentionnelles

`Config::directory` devient le parent applicatif : écriture dans son sous-dossier
`rlogger/jour/instance`. `Rotation` et `InstanceConfig.rotation` sont retirés.
`Runtime::run_path()` est remplacé par `log_root()` et `run_id()`, sans alias.
Les intégrations/mesures doivent sélectionner tous les segments de leur run-id.
Un finalisé n’est plus rouvert ; une capture tardive crée un nouveau segment.

Aucun déplacement d’anciens logs. Racines génériques et formats historiques :
lecture, tailles et actions manuelles selon les permissions du système. La
maintenance automatique exige d’ouvrir exactement la racine RLOGGER 2 privée.
Le frontend 0.5.2 utilise l’ordre décroissant fourni depuis le compagnon 0.3.1.
Le compagnon 0.3.2 ajoute le journal JSON synthétique au générateur d’exemples.
Le compagnon 0.3.0 conserve les mêmes métadonnées mais renvoie les entrées par
nom croissant ; les anciennes routes de lecture restent disponibles.
Ses deux écrans peuvent aussi être copiés dans une application Expo Router web
locale, à condition de servir son build par le même compagnon et sur la même
origine. Cette intégration n’étend pas la compatibilité à iOS ou Android.

## Contrôles et périmètre

Les commandes reproductibles figurent dans le [guide de lancement](../README.md)
et la [CI](../.github/workflows/ci.yml). Les relevés de performances restent
locaux, hors du dépôt public.

Le compagnon mesure contenu et allocation, récupère les actifs abandonnés d’heures
terminées et nettoie les anciens dossiers vides uniquement pour les racines RLOGGER 2
privées. Il autorise les téléchargements et suppressions explicites des fichiers
réguliers et dossiers dans toute racine ouverte, y compris les actifs et formats
historiques, avec précondition d’identité et selon les permissions du système.
Les dossiers se téléchargent en TAR non compressé et leur suppression est récursive.
Les fichiers non vides ne sont jamais supprimés automatiquement. Aucun compte,
serveur distant, service autonome, compression ou politique de rétention ajoutée.

## Limites de validité

- Seule macOS arm64 est exécutée. La publication possède une branche Linux,
  non certifiée ici ; les autres plateformes refusent les primitives non prises
  en charge. Firefox, Chrome et applications natives iOS/Android non validés.
- Allocation Unix = blocs × 512 ; estimation horodatée, pas octets immédiatement
  libérables avec APFS/clones/snapshots. Hardlinks dédupliqués pour l’allocation.
- Scan : profondeur 64, 200 000 entrées totales, 10 000 tailles plus total racine ;
  limites/permissions/mutations donnent un résultat partiel. Pas de plafond des
  offsets de lecture appliqué aux sommes.
- Quatre sessions, 32 connexions TCP, HTTP/1 et WebSocket, fermeture HTTP après
  réponse. Inventaire rafraîchi au plus toutes les 60 s par racine, y compris avec
  `refresh=1` ; invalidations internes regroupées après mutation.
- Un transfert/session et quatre globaux, tickets 30 s/usage unique.
  Transfert par buffers bornés ; annulation possible même si le client cesse de lire.
  Les I/O système bloquées restent non interrompables de force.
- Les verrous sont coopératifs et permanents dans le dossier technique. Ils ne
  protègent pas d’un administrateur local contrôlant/remplaçant les chemins et
  ignorant la coordination. Les fichiers de bail s’accumulent, hors totaux visibles.
- Ordre d’admission par runtime ; aucun ordre global entre processus. Refus
  possibles en saturation, sans équité garantie. Shutdown/Drop peuvent attendre
  le disque ; aucun fsync implicite ni promesse de persistance après panne d’alimentation.
- La récupération préserve seulement les octets déjà sur disque, dernière ligne
  incomplète comprise. Les buffers perdus lors du crash sont irrécupérables.
- Lecture brute bornée à 1 Mio/20 000 lignes, snapshots 256 Kio, blocs 64 Kio,
  offsets v1 jusqu’à 2^53−1. Les réécritures arbitraires restaurant les mêmes
  frontières entre observations restent hors garantie append-only.
- Les relevés courts ne prouvent ni stabilité infinie ni absence d’impact.

Le suivi du même fichier après renommage est livré ; le suivi automatique d’un
segment successeur reste différé (FRONT-028). Parsing structuré/recherche entière,
export JSON, rétention/compression, fsync configurable, timeout d’arrêt, quotas
avancés et plusieurs workers restent hors périmètre.
