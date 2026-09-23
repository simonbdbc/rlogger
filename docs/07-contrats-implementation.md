# Contrats d’implémentation — logger 0.2 / compagnon 0.3

Le contrat horaire [stockage 2](09-migration-arborescence-rlogger.md) remplace les
anciens choix de rotation quotidienne et de dossier par run. Le
[prompt approuvé](10-prompt-gestion-fichiers-horaires.md) et les
[routes de gestion](../front-react-logger/docs/ARCHITECTURE-LOCALE.md#gestion-horaire--extension-030)
complètent les invariants d’admission ci-dessous. Aucun changement de RLOG/1.
Le [contrat du 23 septembre](11-actions-fichiers-dossiers.md) remplace les
restrictions horaires des téléchargements et suppressions explicites.

Choix du 8 septembre 2026 pour GLOBAL-001/002, RLOG-001–004 et FRONT-001–003.
L’exécution de la roadmap autorise l’implémentation locale ; aucune publication externe.

## Organisation et versions

Une racine Cargo : `lib-rust-logger/Cargo.toml`. Le nom public est **RLOGGER**.
Le package et la crate importée sont `rlogger` depuis RLOG-056 ; les noms
`ordered-local-logger` et `ordered_local_logger` désignent l'état antérieur.
Édition 2024, MSRV 1.95, licence MIT.
Chrono 0.4 assure la conversion locale dans le worker ; std fournit mutex,
condvar, thread et fichiers. Tokio est uniquement une dépendance d’exemple/test.
La feature `tracing` ajoute une Layer sans subscriber global.

Le lecteur reste Expo web, Router, React Native Web et gluestack, avec un compagnon
Rust (Axum/Tokio) et WebSocket. Une origine `http://127.0.0.1:4317` sert les assets
du build et l’API. Les versions installées et verrouillées font autorité ; voir COMPATIBILITE.md.
Aucune application externe ne fournit de dépendance ou d’import au projet.

## Admission et ressources Rust

```text
Producteur A : filtre → réservation → format (peut être suspendu) → verrou
Producteur B : filtre → réservation → format → verrou [capture, seq=1, insertion]
Producteur A : reprend → verrou [capture, seq=2, insertion]
Worker      : retire seq=1 → I/O sans verrou → retire seq=2
Fermeture   : verrou [Closing, contrôle shutdown] → formats non admis refusés
```

La réservation ne donne aucune séquence. Une erreur/panic de formatage restitue
son crédit RAII. Le mutex standard assure l’ordre ; aucune atomique de publication
personnalisée. Les compteurs lisibles sous ce verrou forment un instantané cohérent.
Les configurations et collections passées par l’application restent sa propriété.

| Propriétaire | Borne / libération |
| --- | --- |
| Préparation, événement dans la file, premier événement d’un groupe | Réserve pessimiste 8 Kio et un slot, max 8 192 slots / 32 Mio, donc 4 096 réservations simultanées par défaut ; libérée à compaction ou préparation de ligne |
| Occurrences compactées | Compteur u64 par groupe ; ne conservent aucun message individuel |
| Groupes | Au plus une entrée par destination, crédit du premier événement conservé |
| Registre | 32 instances, 128 destinations au total, noms de 64 octets, 128 routes/instance de 256 octets maximum |
| Contextes fixes | Application et instance bornés par la configuration ; 64 champs au plus |
| Contextes d’opérations partagés | Réserve de 8 Kio avant copie, total 1 Mio, libération au dernier clone |
| Copies temporaires de champs | Au plus deux jeux bornés par préparation réservée, surcoût distinct du budget de données |
| Contrôles | Un résumé en file par instance, un compteur de solde ; une barrière globale grâce au mutex de contrôle |
| Writers | 32 buffers de 64 Kio ; fermeture avant remplacement dans le cache |
| Format final | Un événement à la fois, échappement jusqu’à 10 fois la taille texte ; hors budget des captures |
| tracing | 1 024 spans × 8 Kio, 64 captures simultanées, chacune avec plusieurs copies bornées (jusqu’à 32 Kio de données, plus métadonnées), profondeur 32 ; stockage du subscriber externe exclu |

Ces chiffres bornent des données et objets, pas exactement le RSS de l’allocateur.
Les compteurs d’occurrences et séquences sont u64 : une exécution doit être arrêtée
avant leur épuisement. Les valeurs par défaut sont retenues pour cette version après les mesures locales archivées, sans garantie universelle de performance.

Toute saturation incrémente un compteur et une époque de continuité par instance.
Toute autre erreur d’émission coupe aussi la continuité de façon conservatrice.
Un A avant refus ne fusionne pas avec un A admis après refus. Le worker admet les
résumés avec leur propre séquence, jamais avec une séquence/heure rétroactive.
Le résumé de surcharge vise la première destination de l’instance et compte `Full`.
Les autres motifs sont distincts dans les statistiques. Les résumés sont espacés
d’une seconde, avec solde à flush/shutdown ; cette cadence ne bloque pas les événements.

Priorité contexte : application < instance < opération < événement. Clés ASCII,
ordre lexical, dernier insert gagnant ; clés internes réservées refusées. Flottants
finis uniquement, égalité par bits (`-0.0` distinct de `0.0`). Message formaté borné,
un `Display` arbitraire peut néanmoins être lent ou allouer de sa propre initiative.

Routage : destination explicite configurée, puis plus long préfixe de module par
segments `::`, puis destination du nom de crate si déclarée, sinon première destination.
Les clés libres ne créent aucun fichier. Les noms sont ASCII minuscules, aucune
normalisation susceptible de collision. Instance enregistrée jusqu’à la fin du run.

## Temps, fichiers et cycle de vie

Capture monotone et civile sous verrou ; conversion locale côté worker pour l’instant
capturé. Fuseau machine stable requis pendant un run ; un changement manuel nécessite
un nouveau runtime. Les tests de frontières utilisent des offsets contrôlés.
L’heure répétée partage le fichier `-HH.log`, avec coupure du groupe au changement
d’offset ; une heure sautée ne crée pas de fichier. Retour vers un ancien jour : ajout.

Groupes 200 ms depuis la première capture ; flush 100 ms. Le worker sert les échéances
entre événements, même sous flux continu. `LATENCY` = préparation moins première capture
monotone, donc maximum exact du groupe. Elle exclut format avant admission, write,
flush et transport/affichage. Aucun délai universel action → écran n’est promis.

États : Open → Closing → Closed ; toute panne worker/write/flush → Failed.
Politique fail-stop globale, aucun rejeu d’une écriture potentiellement partielle.
`written` compte les occurrences confirmées après un flush réussi ; `lines` inclut
groupes et contrôles. `pending = accepted - written` inclut les données non confirmées
même si des octets sont déjà visibles. Le bilan est conservateur en cas de panne.
`reserved_events` compte des allocations, jamais toutes les occurrences compactées.

Flush couvre les admis avant sa barrière. Shutdown ferme l’admission, draine, termine
groupes et soldes, flush et join. Il est idempotent. Drop du runtime effectue cet arrêt,
peut bloquer sur disque et ne remonte aucune erreur. Drop d’un clone n’arrête rien.
Des refus après shutdown peuvent modifier les stats des handles survivants, pas le
bilan déjà retourné. Flush n’effectue pas fsync et ne garantit pas la persistance.

## Contrat lecteur v1

Session opaque par onglet (4 maximum), une racine et un flux par session. Ouverture
par chemin absolu puis uniquement IDs opaques. Host exact et Origin exact ; toutes
les opérations API sensibles exigent le jeton de session, reçu depuis la même origine.
Racine remplacée uniquement après validation réussie. Racine ancrée à un
descripteur de dossier ; descendants ouverts relativement à lui, sans suivre les
liens symboliques. Identité de la racine et génération du fichier contrôlées
pendant la lecture. Une substitution concurrente de dossier ne doit pas fournir
d’octets hors de la racine ouverte.

Le lecteur est une démo sur une machine à compte fiable : les jetons opaques
servent au protocole et ne prouvent pas l’identité. Une application intégratrice
doit protéger son propre point d’entrée et empêcher l’accès direct au compagnon
depuis des clients non fiables. Tous les fichiers réguliers et dossiers de la
racine ouverte peuvent faire l’objet d’actions explicites selon les permissions
du système, sous précondition d’identité `If-Match`. La maintenance automatique
reste réservée à une racine finale `rlogger` privée avec `.rlogger`, `format` et
`gate` appartenant au compte du compagnon, sans écriture par d’autres comptes
ni ACL macOS étendue. Son refus est visible et préserve la consultation générique.

Positions décimales d’octets jusqu’à 2^53−1, plafond conservé du protocole v1 ; génération opaque ; blocs base64. Snapshot prend une
borne puis lit exactement son intervalle. Le WS reprend à sa fin, avec une seule
tranche non acquittée par client. Un ack confirme génération, intervalle et sélection.
Reset/troncature/remplacement, fichier indisponible et déconnexion sont des contrôles
hors du texte. Le redémarrage du service ou l’expiration de session crée une nouvelle session et revalide la racine ; la sélection redevient manuelle.

256 Kio/snapshot, 64 Kio/bloc direct, 1 Mio/file réseau maximum ; une fenêtre client
bornée à 1 Mio et 20 000 lignes, historique relu sur disque. Arbre paginé à 500, cache 10 000 nœuds maximum,
64 branches observées maximum, révisions contrôlées toutes les secondes ; zéro watcher natif.
Rattrapage 250 ms du fichier actif ; inventaire récursif partagé au plus une fois
toutes les 60 s par racine ouverte, même avec `refresh=1`. Les invalidations après
mutation sont regroupées en un nouveau scan. Inventaire borné à 200 000 entrées
au total, profondeur 64 et cache 10 000 tailles.
UTF-8 incrémental ; aucune newline ajoutée. Réécritures arbitraires entre observations
hors garantie append-only, discontinuités détectées annoncées. Clic manuel prioritaire ;
options successeur, parsing et recherche globale différées (FRONT-028–030).

Références vérifiées : [Chrono Local](https://docs.rs/chrono/latest/chrono/struct.Local.html),
[installation Expo Router](https://docs.expo.dev/router/installation/),
[installation gluestack](https://v5.gluestack.io/ui/docs/home/getting-started/installation).
