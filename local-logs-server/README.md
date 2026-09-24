# Compagnon local Rust — 0.3.0

Binaire autonome Rust 1.95+, Axum/Tokio. Il sert le build Expo et lit les fichiers
sur la même machine. En mode statique, aucun runtime Node ni Python n’est
nécessaire à son exécution. Le mode `--dev` appelle Expo pour exporter le frontend
après les changements de source ; il requiert donc Node 24 et les dépendances npm.
La bibliothèque de journalisation reste indépendante dans `lib-rust-logger`.

Le module `management.rs` gère le stockage horaire version 2 du logger 0.2.0.
Ouvrir exactement la racine `…/rlogger` active la récupération des actifs abandonnés
et le nettoyage des anciennes journées vides si cette racine et les fichiers
`.rlogger`, `format` et `gate` appartiennent au compte courant, ne sont pas
modifiables par les autres comptes et n’ont pas d’ACL macOS étendue. Sinon,
seule cette maintenance automatique est désactivée.
Le lecteur transmet `maintenance: false` dans `POST /api/v1/roots` pour un dossier
externe. Ce choix désactive aussi la récupération et le nettoyage côté Rust si
le chemin sélectionné ressemble à une racine RLOGGER valide ; le champ omis garde
le comportement historique (`true`). Les inventaires restent disponibles.
Les fichiers non vides ne sont jamais
supprimés automatiquement. Le module `actions.rs` permet les téléchargements et
suppressions explicites de fichiers réguliers et dossiers dans toute racine ouverte,
sans condition de format ou d’heure. Les dossiers sont diffusés en TAR non compressé ;
leur suppression récursive reste confinée par des descripteurs, sans suivre de symlink.
Voir le [contrat des actions](../docs/11-actions-fichiers-dossiers.md).
Les listes de l’arbre masquent tous les fichiers et dossiers dont le nom commence
par un point, notamment `.rlogger`, à chaque profondeur. Ce filtre d’affichage
n’exclut pas leur contenu des tailles ni des opérations récursives sur leur parent.
Voir le [contrat horaire](../docs/09-migration-arborescence-rlogger.md) et
les [routes de gestion](../front-react-logger/docs/ARCHITECTURE-LOCALE.md#gestion-horaire--extension-030).

Depuis la racine du dépôt :

```sh
# Préparer l’interface une fois (Node 24 et npm pour le frontend).
cd front-react-logger
npm ci
npm run build
cd ..
# Construire puis lancer le backend Rust.
cargo build --manifest-path local-logs-server/Cargo.toml --release
./local-logs-server/target/release/local-logs-server --dist ./front-react-logger/dist
```

Origine : http://127.0.0.1:4317. `--port 4320` ou `LOCAL_LOGS_PORT=4320` change
le port ; zéro choisit un port libre pour les tests. `--dist` désigne les assets
locaux. Ctrl+C ou SIGTERM ferme HTTP, WebSocket et sessions.
`--poll-ms` règle le rattrapage entre 10 et 5 000 ms (défaut 250 ms).

Pour travailler sur l’interface, lancer `npm run dev` depuis
`front-react-logger/` : le compagnon surveille les sources Expo, relance leur
export et recharge la page après un export réussi. `npm run preview` effectue
un export unique avec le compagnon en mode statique. Les changements du backend
Rust demandent un redémarrage dans les deux modes. Voir le
[guide du lecteur](../front-react-logger/README.md).

HTTP `/api/v1` et WS `local-logs.v1` restent compatibles avec le lecteur précédent :
sessions opaques, racine validée, IDs de nœuds, générations et offsets décimaux.
Le plafond 2^53−1 est conservé pour la compatibilité du protocole, même si Rust lit
les positions avec des entiers u64.

## Ressources et accès disque

HTTP/1 et WebSocket uniquement. Chaque réponse HTTP ferme sa connexion ; les
WebSockets restent ouverts pendant leur session. 32 connexions TCP au maximum,
y compris les WebSockets après upgrade. Quatre
sessions, une opération disque par session, un seul bloc non acquitté par client.
Les lectures passent par `spawn_blocking`, hors des tâches réseau. Les crédits
d’opération sont conservés jusqu’à la fin réelle de la lecture, même si HTTP est annulé.
Les en-têtes HTTP expirent après 5 s ; corps bornés à 8 Kio et 10 s pour l’ouverture.
Une écriture WebSocket bloquée expire après 2 s ; ack absent 10 s : resync/fermeture.

Snapshots 256 Kio, blocs directs 64 Kio ; cache de 10 000 nœuds par session,
500 entrées/page, 64 branches observées, scans limités à 200 000 entrées/dossier.
Aucun watcher natif sur les journaux consultés. Un inventaire récursif borné par
racine ouverte est mutualisé entre sessions et relancé au plus toutes les 60
secondes, y compris après
`refresh=1`. Une mutation gérée invalide ce cache et regroupe les nouveaux scans.
Les handles disque sont
fermés à la fin des opérations. Les lectures système bloquées ne peuvent pas être
interrompues de force ; leur durée dépend du système de fichiers.

Cette interface est une démo locale sur machine à compte fiable. Les jetons de
session sont opaques et ne prouvent pas l’identité d’un utilisateur. Une
application intégratrice doit contrôler l’accès à son propre point d’entrée
et empêcher les clients non fiables de joindre directement ce compagnon.
Les actions gérées supposent aussi que les processus du même compte respectent
la gate RLOGGER : un processus non coopérant du même compte peut échanger un nom
entre la vérification d’identité et une suppression ou publication POSIX.
Host/Origin vérifiés ; consultation et actions explicites sur toute racine ouverte
selon les permissions du système. Seule la maintenance automatique exige une racine
RLOGGER 2 privée. Racine canonicalisée et ancrée à un descripteur, symlinks descendants
refusés, identité contrôlée avant/après ouverture et lecture. macOS arm64 validé ;
aucune certification Windows/Linux dans cette livraison. Les noms non UTF-8 sont
ignorés dans l’arbre. Le contrat append-only et les limites des réécritures
arbitraires restent ceux du [protocole](../front-react-logger/docs/ARCHITECTURE-LOCALE.md).

```sh
cargo test --manifest-path local-logs-server/Cargo.toml
cargo clippy --manifest-path local-logs-server/Cargo.toml --all-targets -- -D warnings
```
