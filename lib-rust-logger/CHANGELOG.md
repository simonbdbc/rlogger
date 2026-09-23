# Changelog

## Distribution de `rlogger` 0.2.0 — 2026-09-23

- RLOG-056 : le package et la crate deviennent `rlogger`. Les intégrateurs
  locaux doivent remplacer la dépendance `ordered-local-logger` et les imports
  `ordered_local_logger` ; le format RLOG/1 et les fichiers existants restent inchangés.
- Feature optionnelle `log` : `LogAdapter` pour une instance, installation
  globale laissée à l'application, `flush` transmis au runtime.
- Métadonnées, guide et exemples de distribution publics ; contrôles du package
  et des consommateurs externes étendus aux features `log` et `tracing`.

## 0.2.0 — 2026-09-13

- Rupture : `Config::directory` parent applicatif, stockage `rlogger/jour/instance`.
- Rotation horaire obligatoire ; suppression de `Rotation` et `InstanceConfig.rotation`.
- `Runtime::run_path()` retiré ; `log_root()` et `run_id()` explicites.
- Segments exclusifs, intervalle UTC, états actif/finalisé/récupéré ; clôture au repos,
  à l’éviction et à l’arrêt, sans réouverture d’un finalisé. Captures tardives conservées.
- Marqueur de stockage 2, bail système par run et coordination courte de racine.
- Texte RLOG/1, admission, crédits, compteurs, flush et fail-stop conservés.
- Anciennes arborescences laissées en place ; lecture générique dans le compagnon.

Le compagnon/lecteur 0.3.0 ajoute tailles et gestion. Pas de renommage Cargo ni publication.

## 0.1.0 — 2026-09-08

- Admission ordonnée et bornée, worker partagé, instances et contextes immuables.
- Routage déclaré, fichiers locaux journaliers/horaires, groupes consécutifs et LATENCY.
- Refus visibles, flush barrière, shutdown drainant et bilan conservateur après panne.
- Layer tracing optionnelle et exemples natif/async synthétiques.
- Protocole de fichier `RLOG/1`, fixture synthétique contractuelle et tests.

Publication externe non effectuée. JSON, rétention/compression, durabilité fsync,
timeout d’arrêt, quotas avancés et multi-worker restent différés.
