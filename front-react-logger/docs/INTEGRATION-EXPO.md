# Intégrer les écrans de logs dans une application Expo web

Le lecteur complet peut rester une application autonome. Pour afficher les mêmes
logs dans une application Expo Router existante, copier uniquement
`front-react-logger/src/log-viewer/` dans `src/log-viewer/` de l’application hôte.
Ce dossier contient les deux écrans, leurs composants, le client HTTP/WebSocket,
le protocole, les préférences et la feuille de style isolée. Son point d’entrée est
`src/log-viewer/index.ts` ; il n’importe aucun fichier de l’application autonome.
Conserver la notice de la [licence MIT](../../LICENSE) lors de la copie.

## Prérequis

- Application **Expo Router web** avec sortie `web.output: "single"`, React et
  React Native Web compatibles avec Expo 57. Le code utilise des API du navigateur
  (`localStorage`, DOM, WebSocket, `<dialog>`) et ne vise pas iOS ou Android.
- Dépendances gluestack du lecteur : `@gluestack-ui/core` 5.0.15 et
  `@gluestack-ui/utils` 5.0.x. Les versions exactes testées de toutes les
  dépendances figurent dans `front-react-logger/package.json`.
- Le [compagnon Rust](../../local-logs-server/README.md) sert **le build de
  l’application hôte et l’API sur la même origine locale**. Les écrans appellent
  `/api/v1` et `/api/v1/live` relativement à cette origine ; ils ne disposent pas
  d’un paramètre d’URL distante.

Le dossier copié fournit `RloggerLogsScreen` et `ExternalLogsScreen`. Chaque écran
crée et ferme sa propre session du compagnon, rouvre automatiquement le dernier
dossier de sa source, affiche l’arbre et le contenu en direct, et garde les actions
explicites de téléchargement et de suppression. Seul `RloggerLogsScreen` demande
la maintenance automatique ; le compagnon vérifie encore l’éligibilité de la
racine privée. L’écran externe ne montre pas d’avertissement de maintenance.

## Ajouter les routes et le menu

Copier le dossier depuis la racine de RLOGGER :

```sh
mkdir -p /chemin/vers/app-hote/src/log-viewer
cp -R front-react-logger/src/log-viewer/. /chemin/vers/app-hote/src/log-viewer/
```

Créer `app/logs/rlogger.tsx` dans l’application hôte :

```tsx
import { RloggerLogsScreen } from "../../src/log-viewer";

export default function RloggerLogsRoute() {
  return (
    <div style={{ height: "100dvh" }}>
      <RloggerLogsScreen />
    </div>
  );
}
```

Créer `app/logs/external.tsx` :

```tsx
import { ExternalLogsScreen } from "../../src/log-viewer";

export default function ExternalLogsRoute() {
  return (
    <div style={{ height: "100dvh" }}>
      <ExternalLogsScreen />
    </div>
  );
}
```

Ajouter deux liens dans **le menu existant** de l’application hôte, sans reprendre
le menu du lecteur autonome :

```tsx
import { Link } from "expo-router";

<Link href="/logs/rlogger">Journaux RLOGGER</Link>
<Link href="/logs/external">Journaux externes</Link>
```

Adapter la hauteur des conteneurs d’écran à l’en-tête et au menu de l’application
hôte : le lecteur a besoin d’une hauteur définie pour faire défiler séparément
l’arbre et le contenu. Sa feuille `viewer.css` ne cible que `.rlogger-viewer` et
ne modifie ni `html`, ni `body`, ni les boutons de l’application hôte. Chaque écran
inclut son fournisseur gluestack et sa feuille de style ; le layout hôte n’a rien
à importer pour le lecteur.

Les propriétés optionnelles des deux écrans sont `initialDirectory` (chemin
utilisé à la place du chemin mémorisé lors du montage), `onDirectoryChange`
(suivi du texte saisi si le menu hôte conserve l’état de ses routes) et
`onResetPreferences` (appelé après effacement des deux chemins mémorisés et de
la largeur de l’arbre). Sans ces propriétés, chaque écran lit son dernier dossier
dans le `localStorage` du navigateur.
Les clés `local-logs-path-rlogger`, `local-logs-path-external` et
`local-logs-width` ne contiennent pas le contenu des fichiers.

## Servir l’application hôte

Exporter le frontend de l’application hôte avec `npx expo export --platform web`
(ou son script de build équivalent), puis lancer le compagnon depuis la racine du
dépôt RLOGGER :

```sh
cargo run --manifest-path local-logs-server/Cargo.toml --release -- \
  --dist /chemin/vers/app-hote/dist
```

Ouvrir `http://127.0.0.1:4317/logs/rlogger` ou
`http://127.0.0.1:4317/logs/external`. La sortie Expo `single` permet au compagnon
de servir `index.html` pour ces routes. Aucun serveur Node n’est lancé : Node sert
à installer, exporter et tester le frontend. Un autre port local peut être choisi
avec `--port` ou `LOCAL_LOGS_PORT`.

Le compagnon accepte uniquement son Host et son Origin locaux. Une application
servie sur une autre origine ne peut pas utiliser ces écrans sans revoir le
transport et la protection d’accès. Le jeton de session ne constitue pas une
authentification utilisateur : l’application intégratrice doit contrôler son
point d’entrée et empêcher les clients non fiables d’atteindre directement le
compagnon. Les chemins sont ceux du **Mac qui exécute le compagnon**, et les
actions manuelles suivent ses permissions système.

Pour reprendre tout le frontend autonome, utiliser plutôt
[`front-react-logger/`](../README.md) et son `npm run build` / `npm start`.
