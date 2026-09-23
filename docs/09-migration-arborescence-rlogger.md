# Stockage horaire RLOGGER 2 — migration du logger 0.2.0

Implémenté dans le logger 0.2.0 et le compagnon/lecteur 0.3.0. Le
[contrat des actions](11-actions-fichiers-dossiers.md) décrit les téléchargements
et suppressions manuels.

## Emplacement et ruptures d’API

`Config::directory` est le **parent applicatif**, pas le dossier final des logs.
Pour écrire dans `/var/log/mon-app/rlogger`, passer `/var/log/mon-app`.

```text
<application>/rlogger/
├── .rlogger/
│   ├── format                 # RLOGGER storage 2
│   ├── gate                   # coordination permanente
│   └── <run-id>.lock          # bail système exclusif du writer
└── <YYYY-MM-DD>/
    └── <instance>/
        └── <destination>-HH-<run-id>-h<début-heure-UTC>-s<segment>.active.log
```

`run-id = nanosecondes-epoch-pid-compteur`, alloué par création exclusive du bail.
`h` contient un début d’heure en secondes Unix signé ; HH et le jour sont locaux.
Le compteur de segments est croissant par runtime et commence à 1. La création
exclusive empêche tout écrasement. Noms d’instance/destination validés comme avant.

Supprimer `InstanceConfig.rotation` et les imports `Rotation` chez les consommateurs.
La rotation est toujours horaire. Remplacer `Runtime::run_path()` par `log_root()`
pour la racine commune et `run_id()` pour isoler les fichiers d’un runtime.
Aucun alias de l’ancienne API : un chemin de run isolé n’existe plus.
À la date de cette migration, le package Cargo s'appelait
`ordered-local-logger` et la crate `ordered_local_logger`. RLOG-056 les a
ensuite renommés en `rlogger` sans nouvelle migration des fichiers.
Le format de **ligne** reste RLOG/1 ; seul le contrat de stockage change.

## Clôture et événements retardés

| Suffixe | Sens | Maintenance automatique |
| --- | --- | --- |
| `.active.log` | Writer capable d’écrire, ou interruption non récupérée | Récupération possible après fin d’heure et libération du bail |
| `.log` | Buffer vidé, writer fermé, publication normale | Aucun retrait automatique du fichier |
| `.recovered.log` | Fichier abandonné récupéré sous verrou de run | Aucun retrait automatique du fichier ; indication de récupération |

Les téléchargements et suppressions explicites portent sur tous les fichiers
réguliers et dossiers de la racine ouverte, y compris les fichiers actifs et les
anciens formats. Ils ne dépendent ni du suffixe ni de l’heure.

Le worker termine les groupes concernés, flush, ferme puis renomme dans le même
dossier sans remplacer une cible. Il clôt à l’échéance horaire même sans événement,
à l’éviction LRU et à l’arrêt sain après drainage. L’échéance dépend du réveil du
worker et des I/O ; aucune ponctualité temps réel n’est promise.
`flush()` reste une barrière de visibilité, pas systématiquement une clôture.

Une capture tardive conserve son jour et son heure. Une heure revisitée après
clôture crée un nouveau segment actif ; aucun fichier finalisé n’est rouvert.
Clôturer ne prétend pas que la file entière a été consommée. Groupes et crédits
sont réconciliés une fois, avec les compteurs et le comportement fail-stop.
Les heures d’hiver répétées ont des intervalles UTC distincts ; une heure sautée
ne crée pas de fichier vide. Recréer le runtime après changement manuel du fuseau.

Aucun accès disque ajouté à l’appel producteur. Disque et verrous sont dans le
worker/initialisation ; aucun réseau, fsync ou runtime async ajouté au logger.
Un échec de flush/publication protège les fichiers restés actifs ; aucune écriture
éventuellement partielle n’est rejouée et le bilan d’erreur reste accessible.

## Coordination et récupération

Le bail OS exclusif du run couvre toute sa capacité à écrire, évictions comprises.
Le compagnon acquiert ce même bail exclusivement pour récupérer un actif d’une
heure terminée. PID absent, délai ou mtime stable ne prouvent jamais un arrêt.
Renommage seul : octets et dernière ligne incomplète conservés. Les buffers qui
n’avaient pas atteint le disque ne sont pas récupérables. Un bail inaccessible ou
une cible existante laisse le fichier protégé avec diagnostic ; reprise idempotente.

Le verrou de racine sérialise les courtes créations, clôtures, récupérations et
suppressions de dossiers. Jamais détenu pendant un scan complet ou un transfert.
Les fichiers de verrou ne sont pas supprimés : retirer un verrou encore référencé
créerait deux domaines de coordination. Leur accumulation est distincte des logs
visibles et n’entre pas dans les totaux affichés.

Les opérations relatives utilisent des descripteurs de dossiers, O_NOFOLLOW,
création exclusive, publication atomique sans remplacement et suppression de
dossier vide uniquement pour le nettoyage automatique. Les actions manuelles
sur dossiers suivent le [contrat actuel](11-actions-fichiers-dossiers.md).
Composants et identité de racine revérifiés.
La maintenance automatique exige la racine finale `rlogger`, ainsi que sa racine,
`.rlogger`, `format` et `gate` privés, appartenant au compte du compagnon, sans
écriture par d’autres comptes ni ACL macOS étendue. Sinon, le lecteur garde la
consultation générique et les actions explicites selon les permissions du système ;
il affiche la raison du refus de maintenance.
Les verrous sont coopératifs : un administrateur local peut contourner le protocole.
Les [verrous de File](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
sont stables depuis Rust 1.89, compilés/testés avec la MSRV 1.95.
Publication macOS/Linux ; seule macOS arm64 a été exécutée.

## Anciennes données et responsabilités

Aucun déplacement/renommage automatique des anciens runs. Racines génériques et
anciens formats restent consultables avec tailles et actions explicites.
Ouvrir exactement `…/rlogger` avec les permissions privées requises pour activer
la maintenance automatique ; son parent ou un sous-dossier conservent la
consultation générique et les actions explicites.

Le compagnon possède identification, inventaires, éligibilité, transfert, suppression,
récupération et nettoyage. La lib possède stockage, clôture et primitives de
coordination partagées. L’application choisit le parent et initialise le logger ;
elle n’a pas à réimplémenter les tailles. Le frontend présente les intentions,
le serveur décide les permissions. Pas de compression, rétention des fichiers
non vides, service autonome ou publication externe.
