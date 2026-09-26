# Architecture locale et contrat de lecture v1

Lecteur 0.5.2, compagnon Rust 0.3.2, logger 0.2.0 ; protocole v1 étendu pour
la gestion horaire.
[Guide de lancement](../README.md), [types partagés](../src/log-viewer/protocol.ts),
[intégration Expo web](INTEGRATION-EXPO.md).

## Responsabilités et lancement

Expo web / Router / gluestack composent le lecteur. Le compagnon Rust (Axum/Tokio)
sert le build et l’API sur `http://127.0.0.1:4317` ; port configurable par environnement.
`npm run build` puis `npm start`, Ctrl+C pour fermer le service et ses connexions.
`npm run dev` lance un export Expo, puis le serveur Rust surveille les sources :
il exporte vers l’un de deux dossiers temporaires ignorés par Git et remplace le
build servi après un export réussi. Un script injecté seulement dans la page de
développement consulte `GET /__dev/version` et recharge la page quand la version
change. En cas d’échec de l’export, la version servie ne change pas. Les appels
API restent sur la même origine. `npm run preview` effectue un seul export ; un
changement du backend demande toujours un redémarrage.
Aucun proxy, CDN ou tunnel nécessaire. La crate n’a aucune dépendance à ce service.

HTTP et WS vérifient Host/Origin exacts ; les opérations sensibles exigent un jeton
aléatoire propre à chaque onglet. Quatre sessions maximum, une racine et une sélection
par session. Lecture générique ; gestion de fichiers RLOGGER 2 explicitement reconnue. Les logs ne sont jamais interprétés comme HTML.
Le jeton est un état de protocole, pas une preuve d’identité. Cette démo suppose un
compte local fiable. Une application intégratrice doit contrôler l’accès à son
point d’entrée et empêcher l’accès direct au compagnon depuis des clients non fiables.

## API livrée

| Opération | Route / données | Résultat |
| --- | --- | --- |
| Santé | `GET /api/v1/health` | `version`, `serverId` |
| Nouvelle session | `POST /api/v1/session`, Origin requis | Jeton opaque, version et identité du serveur |
| Valider / fermer une session | `GET` / `DELETE /api/v1/session` | Session active / fermeture des ressources |
| Racine | `POST /api/v1/roots`, `{absolutePath, maintenance?: boolean}` | `rootId`, `nodeId`, chemin canonique, `managedReason` nullable |
| Enfants | `GET /api/v1/roots/{rootId}/entries?parentId=…&cursor=…` | Entrées, révision et cursor suivant |
| Tranche | `GET /api/v1/roots/{rootId}/files/{fileId}/content` | Fin du fichier par défaut |
| Historique / reprise | Même route, `before=…` ou `after=…`, `generation=…` | Intervalle contigu de la génération demandée |
| Flux | `WS /api/v1/live`, sous-protocole `local-logs.v1.<token>` | Abonnement, chunks, ack et contrôles |
| Fermer la racine | `DELETE /api/v1/roots/{rootId}` | Libération de l’état ; aucun fichier supprimé |

Le jeton HTTP passe dans `x-local-session`. Les IDs opaques proviennent de l’arbre ;
un chemin arbitraire n’est accepté qu’à l’ouverture de racine. JSON d’erreur :
`code` et `message`, avec statut HTTP. NOT_FOUND, PERMISSION, ROOT_CHANGED,
GENERATION_CHANGED, CURSOR_INVALID, FORBIDDEN, LIMIT et SESSION_EXPIRED sont distincts.

L’écran **Journaux RLOGGER** envoie `maintenance: true`, l’écran **Journaux
externes** envoie `maintenance: false`. L’absence du champ conserve le comportement
historique (`true`). La maintenance automatique exige à la fois ce choix et la
validation de la racine privée RLOGGER v2. Les inventaires restent disponibles
dans les deux parcours ; les tâches d’inventaire sont distinctes selon ce choix,
afin qu’une consultation externe ne déclenche aucune récupération ni nettoyage.
Le changement de parcours ferme la session courante et sa racine via
`DELETE /api/v1/session`, puis ouvre automatiquement le chemin mémorisé pour ce
parcours. Au démarrage, le parcours
sélectionné ouvre aussi son chemin mémorisé. Chaque ouverture revalide la racine
côté Rust ; un chemin absent ou non autorisé reste affiché avec son erreur.
Les chemins sont des préférences du navigateur local ; aucun chemin d’application
tierce n’est fourni par le dépôt.

## Snapshot, flux et reprise

Une tranche contient génération, `start`, `end`, taille et `data` base64. Les positions
sont des chaînes décimales d’octets, maximum 2^53−1 (plafond de compatibilité du protocole v1).
Le snapshot fixe une borne de fin puis boucle sur le nombre d’octets effectivement lus.
Le WS reprend à `end`, rattrapant tout append survenu entre snapshot et abonnement.
Le client associe chaque sélection à une révision ; les réponses HTTP/WS périmées
sont ignorées. Changer de racine invalide aussi les opérations de l’ancienne racine.
Une ouverture échouée conserve la précédente.

Une seule tranche non acquittée par session. Ack = révision, génération et fin exacte.
Les offsets confirmés avancent ; aucun contrôle n’est inséré dans le texte.
`reset`, `file-unavailable`, `resync-required`, `tree-changed`, `heartbeat` et
`subscribed` décrivent exclusivement le transport ou le cycle de vie.
Après coupure : backoff de 500 ms à 5 s, reprise au dernier intervalle client consommé.
Après redémarrage ou expiration : nouvelle session, dossier revalidé, sélection manuelle.
Le heartbeat toutes les 5 s permet de fermer un flux silencieux depuis plus de 15 s.

## Disque et générations

La racine absolue est canonicalisée puis ancrée à un descripteur. Aucun symlink
descendant n’est suivi ; les ouvertures et lectures de dossier sont relatives à ce
descripteur. Chaque ouverture vérifie l’identité de la racine puis celle du handle avant/après lecture ;
O_NOFOLLOW / O_NONBLOCK et fichier régulier requis. Les lectures disque utilisent spawn_blocking avec un verrou par session ; le réseau reste asynchrone. Le service ne protège pas contre
un administrateur local contrôlant le processus ou réécrivant tous les chemins concurremment.

Identité/inode, taille et empreinte de 64 octets permettent de détecter troncature et
remplacement, y compris certains remplacements de même taille. Renommage/suppression
laissent la sélection identifiable et observée ; un retour au même chemin est relu
avec contrôle de génération. Les générations ne sont jamais concaténées.
Les réécritures arbitraires qui restaurent la même taille et les mêmes frontières
entre deux observations restent hors garantie append-only.

## Budgets et propriétaires

| Ressource | Borne / libération |
| --- | --- |
| Sessions | 4 globales, fermeture explicite ou expiration après 2 min déconnectées |
| Connexions TCP | 32 globales ; délais HTTP bornés |
| Requêtes | Une opération spawn_blocking par session, queue client 16 maximum |
| Racine / arbre | 10 000 IDs par session, 500 entrées/page ; cache ancien évictable |
| Exploration | 200 000 entrées scannées/dossier, profondeur 64, chemin 4 096 caractères |
| Observations | 64 branches ouvertes ; contrôle métadonnées toutes les secondes, zéro watcher natif |
| Snapshot / historique | 256 Kio par lecture, descripteur fermé immédiatement après |
| Direct | 64 Kio/bloc ; polling 250 ms au repos, rattrapage immédiat après ack |
| Réseau | Un bloc non acquitté, buffer de sortie borné à 1 Mio ; ack absent 10 s = fermeture/resync |
| Vue | 1 Mio d’octets, 20 000 lignes physiques ; projection JSON en lignes visuelles, rendu DOM virtualisé |
| Transitoires | Buffers bruts, base64 (~4/3), texte UTF-16 et copies de fenêtre s’ajoutent aux octets utiles |

Ces plafonds ne sont pas un plafond RSS global. Les bibliothèques, le navigateur,
l’allocateur et les métadonnées ont leur propre coût.
L’arbre est paginé par noms décroissants ; la première observation invalide le snapshot
initial pour couvrir les créations survenues avant installation de l’observation.
Les inventaires récursifs de tailles/maintenance sont mutualisés par racine ouverte,
bornés à 200 000 entrées au total et relancés au plus une fois toutes les 60
secondes par racine, même avec `refresh=1`. Une mutation gérée invalide le cache
et regroupe ses nouveaux scans. Replier libère
l’observation de branche ; fermer/changer de racine libère sa référence d’inventaire.

## Rendu et interaction

L’arbre masque à toute profondeur les fichiers et dossiers dont le nom commence
par un point, dont `.rlogger`. Ils restent inclus dans les tailles et opérations
récursives sur leur parent. Chaque entrée visible possède à droite les mêmes
icônes de téléchargement et de suppression, disponibles sans sélectionner le
fichier ni déplier le dossier, selon son éligibilité et l’opération en cours.
Les boutons ont une infobulle et un libellé accessible précisant la cible.
La suppression demande confirmation ; Annuler ou Échap rend le focus à l’icône.
Le lecteur ne duplique pas les boutons, mais conserve les indications de récupération
et de refus. Agir sur un autre fichier ne change pas la sélection en lecture.

UTF-8 décodé sans newline ajoutée ; fragments multioctets conservés, CRLF préservés.
Octets invalides signalés et remplacés par �. Une ligne géante reste soumise au plafond.
Pages anciennes contiguës, ancre conservée au prepend ; début du fichier atteignable.
En lecture historique les nouveaux blocs sont acquittés sans accumulation ; une
indication de nouveaux octets permet de relire la fin sur disque. Copier porte sur
le texte rendu, jamais sur tout le fichier non chargé.

`viewer-store.ts` conserve la fenêtre de texte décodé comme référence. Dans le
navigateur, `rlog-display.ts` projette les lignes RLOG/1 complètes et valides
dont le message est du JSON : décodage des seuls échappements RLOG/1, puis
`JSON.parse` et indentation sur deux espaces. Les lignes non reconnues et les
fragments aux bords de la fenêtre restent bruts. **Journaux externes** ne lance
pas cette projection. Le bouton du parcours RLOGGER commute entre projection et
texte brut sans requête ni nouvelle session ; aucun octet du fichier ne change.
Chaque ligne physique possède un indice de début dans les lignes visuelles.
Virtualisation, largeur et hauteur utilisent les lignes visuelles ; lors d’un
chargement d’historique, le nombre de lignes physiques ajoutées est converti
par cet indice pour préserver l’ancre de défilement. Le texte est rendu par
React, jamais interprété comme HTML, ANSI ou script.

Le suivi du scroll s’applique seulement en bas. Le clic manuel garde la sélection,
y compris après rotation. Clavier, séparateur ajustable, panneaux indépendants ;
à 390 px, arbre au-dessus du lecteur. Les deux chemins, le parcours sélectionné
et la largeur sont mémorisés localement.
FRONT-028–030 (successeur, parsing structuré complet, recherche entière) restent
différés. La projection JSON par ligne n’interprète ni `part`/`last_part` ni les
groupes et ne clôt pas FRONT-029 ; le texte brut reste accessible.

## Gestion horaire — extension 0.3.0

Le lecteur 0.5.2 utilise le compagnon 0.3.2 pour ces DTO additionnels ; HTTP/WS
et les tranches existantes conservent la version 1. Le logger reste indépendant.

| Opération | Route | Contrat |
| --- | --- | --- |
| Inventaire | `GET /api/v1/roots/{rootId}/statistics`, option `refresh=1` | `items` par chemin relatif, `sampledAt`, `busy`, `partial`, `notices` |
| Métadonnées | `GET /api/v1/roots/{rootId}/files/{fileId}` | Entry actuelle, y compris après finalisation |
| Préparer | `POST /api/v1/roots/{rootId}/files/{fileId}/download` | Origin, session et `If-Match: identity`, retourne `{url}` |
| Télécharger | `GET /api/v1/downloads/{ticket}` | Usage unique, 30 s, en-tête attachment, nom UTF-8 encodé ; flux 64 Kio |
| Supprimer | `DELETE /api/v1/roots/{rootId}/files/{fileId}` | Origin, session, `If-Match: identity` obligatoire, `{deleted:true}` après succès |

Entry ajoute `identity` (dev/inode, taille, mtime et ctime), `allocatedSize`
nullable, `state` (active/closed/recovered/legacy/unknown), `eligible` et `reason`.
Depuis le 23 septembre, toute entrée régulière de la racine ouverte peut être
téléchargée/supprimée, dossiers compris, indépendamment du marqueur et de l’heure.
`eligible`/`reason` reflètent disponibilité, verrouillage et confinement.
Les routes historiques `/files/{fileId}` acceptent aussi un ID de dossier pour
les métadonnées et actions ; `/content` reste réservé aux fichiers.
Le [contrat des actions](../../docs/11-actions-fichiers-dossiers.md) précise les bornes,
archives TAR et suppressions récursives. Seule la maintenance automatique conserve
les conditions de racine RLOGGER privée.
`size` est le contenu du fichier ; les dossiers utilisent les sommes d’inventaire.
Chaque `items[path]` contient `content`, `allocated` nullable et `partial`.
Toutes les tailles sont des chaînes décimales ; Rust additionne en u128 et le
frontend utilise BigInt, indépendamment du plafond d’offset v1.

`FILE_PROTECTED`/`FILE_BUSY`/`FILE_CHANGED` : 409 ; précondition absente : 428
`PRECONDITION_REQUIRED` ; ticket expiré, invalidé ou réutilisé : 410 `DOWNLOAD_EXPIRED`.
Une erreur ne déclenche aucun retrait optimiste de l’entrée. Les identifiants
opaques restent propres à une racine de session ; aucun chemin arbitraire d’action.

Un ticket par session, remplacé par une préparation suivante ; jamais journalisé.
Un transfert par session, quatre globaux. Le retrait attend au plus 2 s derrière
l’opération disque de session, puis conserve seulement le descripteur partagé et
les permis de transfert. Un tampon de transit de 64 Kio relie copie disque et corps HTTP.
Une tâche bornée surveille fermeture/racine toutes les 50 ms même si le client ne
lit plus ; arrêt du compagnon également pris en compte. Un verrou partagé du fichier interdit sa suppression
depuis tous les compagnons coopérants. Fin, annulation, session fermée ou racine
changée libèrent les ressources ; un transfert annulé peut avoir déjà envoyé des octets.

Un job partagé par identité de racine : profondeur 64, 200 000 entrées **par scan**,
10 000 tailles de nœuds plus le total racine, 16 diagnostics ; un scan simultané.
Une table dev/inode bornée par les entrées parcourues déduplique l’allocation,
hors cache d’IDs de navigation. Travail disque hors tâches réseau, pause toutes
les 128 entrées, annulation entre opérations lorsque la dernière racine est fermée.
Les I/O système bloquées ne peuvent pas être interrompues de force. Le client
consulte le cache toutes les 5 s (500 ms durant calcul) ; cela ne lance pas un scan
toutes les 5 s. Actualisation explicite et suppression invalident le cache.

Maintenance seulement sur racine RLOGGER 2 reconnue : bail exclusif du run avant
récupération, coordination courte de racine avant mutation, aucun scan/transfert
sous ce verrou. Jour local strictement passé et rmdir seulement ; cachés/inconnus
préservent les dossiers. L’allocation Unix est `blocks × 512`, jamais un calcul
de l’espace immédiatement récupérable. Le WS `file-renamed` porte révision et Entry ;
il conserve ID/génération et fenêtre du même inode, sans suivre le segment suivant.

Références système : [verrous File](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
et [blocs alloués Unix](https://doc.rust-lang.org/std/os/unix/fs/trait.MetadataExt.html#tymethod.blocks).
