# Format de fichiers RLOG/1

Propriétaire : RLOGGER 0.2.0 (grammaire RLOG/1 inchangée). Le package
`rlogger` succède à `ordered-local-logger` sans modifier les lignes RLOG/1.
Le mode brut du lecteur reste indépendant du format. Le préfixe de chaque ligne
est `RLOG/1`. Dans le navigateur, le parcours **Journaux RLOGGER** peut
présenter le message JSON d’une ligne complète et valide avec une indentation
et un en-tête lisible ; **Journaux externes** et le bouton **Voir le texte brut**
conservent la représentation textuelle. Le format disque et le protocole ne
changent pas. Le parseur structuré complet FRONT-029 reste différé.

Exemples exacts validés par `shared_contract_fixture_matches_writer` :

```text
RLOG/1 2026-09-08T12:00:00.000+02:00 INFO [LATENCY — 0ms] [seq=3 instance=test source="fixture:fixture.rs:1" call_id="c1"] B
RLOG/1 2026-09-08T12:00:00.000+02:00 INFO [LATENCY — 0ms] [seq_first=4 seq_last=5 count=2 last=2026-09-08T12:00:00.000+02:00 instance=test source="fixture:fixture.rs:1" call_id="c1"] A x2
RLOG/1 2026-09-08T12:00:00.000+02:00 ERROR [LATENCY — 0ms] [seq=6 instance=test source="fixture:fixture.rs:1" call_id="c1"] Erreur 🦀\nligne\t\u{1b}
```

Référence unique : [fixture synthétique](../lib-rust-logger/fixtures/v1/rust.log),
consommée par les tests Rust et référencée par le générateur du lecteur.

## Grammaire et précision

1. `RLOG/1`, espace, capture civile ISO 8601 à la milliseconde (fraction tronquée),
   offset local ; `Z` est possible si le fuseau local est UTC.
2. Niveau TRACE/DEBUG/INFO/WARN/ERROR.
3. `[LATENCY — Nms]` : durée monotone entière, tronquée en millisecondes,
   calculée au moment de préparer la ligne. Maximum exact du groupe, avant arrondi.
4. Bloc de métadonnées internes, puis champs utilisateur dans l’ordre lexical.
5. Message échappé ; suffixe ` xN` pour les groupes de plus d’une occurrence ; LF.

Événement simple : `seq`. Groupe : `seq_first`, `seq_last`, `count`, `last`.
Les bornes ne constituent jamais une plage contiguë garantie : des séquences d’autres
destinations peuvent s’intercaler. Le compteur est le nombre d’occurrences de ce groupe.
Un message utilisateur peut lui-même se terminer par `xN` ; un parseur doit lire les
métadonnées `count`, pas inférer un groupe depuis ce seul suffixe.

L’instance réelle est distincte des champs ; `source="module:file:line"` décrit
l’origine et le thread facultatif est `thread="ThreadId(...)"`. Les valeurs utilisateur
texte sont entre guillemets ; booléens et entiers sont typés en décimal ; flottants
finis avec représentation Rust Debug, `-0.0` distinct de `0.0`.

Échappement : antislash `\\`, guillemet `\"`, LF `\n`, CR `\r`, tabulation `\t`,
autres contrôles et séparateurs U+2028/U+2029 `\u{hex}`. La présentation JSON
du lecteur décode ces échappements uniquement pour une ligne reconnue, puis
applique `JSON.parse` ; en cas d’échec, elle garde la ligne brute. Le navigateur
n’interprète jamais le contenu comme terminal, HTML ou code.

Contrôle de surcharge : niveau WARN, `source=logger`, message `OVERLOAD refused=N`,
séquence et capture propres au résumé, LATENCY présente. Il ne reconstruit pas les
événements refusés et ne prétend pas avoir été émis à leurs instants.

## Frontières de fichiers

Stockage version 2 : `rlogger/<jour>/<instance>/`
`<destination>-HH-<run-id>-h<début-heure-UTC>-s<segment>.active.log`.
Clôture normale en `.log`, récupération en `.recovered.log`.
Le [contrat de migration](09-migration-arborescence-rlogger.md) fait autorité.
Rotation obligatoire ; pas de réouverture après finalisation. Capture tardive :
nouveau segment de l’heure capturée. Heure répétée : intervalle UTC distinct.
Une heure sautée ne crée aucun fichier artificiel.

Rotation, offset, époque de refus, source, niveau, contexte, message, durée de groupe
ou barrière peuvent séparer les groupes. La file restante n’est pas considérée vide
à la clôture d’un writer. Le texte et les séquences RLOG/1 restent inchangés.
Aucune suppression automatique de fichiers non vides, aucune compression.

## Évolution

Tout changement des champs internes, de l’échappement ou de la signification des
groupes exige une revue d’impact et une version de format explicite. Modifier une
rotation impose les tests de fichiers et de lecteur. Modifier HTTP/WS impose les
tests du protocole v1 des deux côtés. La lecture brute doit rester utilisable pour
un format inconnu ; aucun numéro de séquence métier n’est un offset d’octets.
