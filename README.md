# dubSuite

Lanceur des applications dub. Il affiche le catalogue, installe le programme choisi, l'ouvre, le met à jour quand une Release GitHub plus récente existe, et peut le désinstaller. Chaque application reste son propre dépôt.

```powershell
cargo run
```

La version affichée en haut de la fenêtre est celle de `Cargo.toml`.

## Utilisation

Chaque carte montre le nom, la description, la dernière version publiée et, le cas échéant, la version déjà installée.

- **Installer** télécharge la Release, vérifie la somme SHA-256 quand elle est jointe, puis place les fichiers dans le dossier de l'application.
- **Ouvrir** lance le programme installé.
- **Mettre à jour** apparaît quand la Release est plus récente que la copie locale. Si la copie locale est plus récente, la carte le signale et ne propose pas de revenir en arrière.
- **Désinstaller** demande confirmation, puis efface le dossier de l'application, son ancienne copie et les paquets mis en cache pour elle.
- Le champ de recherche filtre par nom, description ou identifiant.
- **Actualiser** relit le catalogue et interroge de nouveau GitHub. Un seul téléchargement à la fois ; **Annuler** l'interrompt.

Les programmes installés sont rangés dans `%LOCALAPPDATA%\DubSuite\apps\<id>\`. Sous Linux, le dossier est `$XDG_DATA_HOME/dubsuite`, ou `~/.local/share/dubsuite`.

| Fichier | Rôle |
| --- | --- |
| `settings.json` | Adresse du catalogue et jeton GitHub |
| `state.json` | Versions installées |
| `catalog.cache.json` | Dernier catalogue en ligne reçu |
| `cache\` | Paquets en cours de téléchargement |
| `update-error.txt` | Erreur de la dernière mise à jour du lanceur, affichée au démarrage suivant |

## Ajouter une application

Une entrée dans `catalog.json` suffit. Le lanceur relit ce fichier au démarrage.

```json
{
  "id": "dubphoto",
  "name": "dubPhoto",
  "mark": "Ph",
  "summary": "Courte description affichée sur la carte",
  "repo": "Creations-Daniel-Dube/dubPhoto",
  "exe": "dubPhoto.exe",
  "exe_linux": "dubPhoto",
  "accent": "#7dcea0"
}
```

`id` sert de nom de dossier. Lettres, chiffres, tiret et souligné seulement, 64 caractères au plus. L'identifiant `launcher` est réservé. Le catalogue accepte au plus 100 applications.

`mark` est le sigle affiché quand l'application n'a pas d'icône intégrée au lanceur (trois caractères au plus). `accent` est une couleur `#rrggbb` pour ce sigle. `exe_linux` est le nom du binaire hors Windows ; s'il est absent, le lanceur retire le suffixe `.exe`.

Le catalogue inclus liste aujourd'hui dubDSR (`Creations-Daniel-Dube/dubDSR`) et dubFocus (`Creations-Daniel-Dube/dubFocus`).

## Publier une version

Pour que la carte propose **Installer**, le dépôt doit avoir une Release GitHub. Le tag doit contenir la version semver de `Cargo.toml`, par exemple `v0.2.10` ou `dubDSRv1.2.1`.

Le lanceur prend la dernière Release et choisit le paquet de la plateforme en cours. Sur Windows x64, il cherche d'abord un nom qui contient `windows-x64`, puis `windows` :

`dubPhoto-0.2.10-windows-x64.zip`

S'il n'y a qu'un seul `.zip`, il est utilisé même sans ce suffixe. S'il n'y a aucun zip, un `.exe` seul, comme `dubdsr1.2.1.exe`, est accepté de la même façon. Plusieurs paquets pour la même plateforme provoquent une erreur. Un paquet de plus de 1 Gio est refusé.

Dans un zip, `exe` est cherché par son nom, y compris dans un sous-dossier. S'il est introuvable et que l'archive ne contient qu'un seul `.exe`, celui-ci est installé.

Une somme SHA-256 jointe à la même Release est vérifiée avant l'installation. Le fichier peut s'appeler `checksums.txt`, `sha256sums`, ou porter le nom du paquet suivi de `.sha256`. Le format `sha256sum` convient ; un fichier qui ne contient que les 64 caractères hexadécimaux aussi.

```powershell
gh release create v0.2.10 Distribution/dubPhoto-0.2.10-windows-x64.zip Distribution/checksums.txt --repo Creations-Daniel-Dube/dubPhoto --title "dubPhoto 0.2.10" --generate-notes
```

## Catalogue en ligne

L'adresse est prise dans cet ordre : la variable `DUBSUITE_CATALOG_URL`, puis celle enregistrée dans **Catalogue…**, puis le champ `catalog_url` du fichier local. Sans adresse, le lanceur utilise le `catalog.json` du répertoire courant, celui placé à côté du programme, celui du projet quand on lance depuis `target`, puis le catalogue inclus dans le programme.

Pour qu'une nouvelle application apparaisse sans recompiler le lanceur, publiez ce fichier et indiquez son adresse dans **Catalogue…** :

`https://raw.githubusercontent.com/Creations-Daniel-Dube/dubSuite/main/catalog.json`

L'adresse doit commencer par `https://`. Si le téléchargement échoue, la dernière copie réussie (`catalog.cache.json`) est utilisée, sinon le catalogue local.

## Mise à jour du lanceur

Dans `catalog.json`, renseignez le dépôt du lanceur :

```json
"launcher": { "repo": "Creations-Daniel-Dube/dubSuite", "exe": "dubSuite.exe" }
```

Quand une Release plus récente existe, une bande l'annonce. **Mettre à jour** télécharge le zip, le prépare à côté du programme en cours (`dubSuite.exe.new`), remplace l'exécutable, puis relance le lanceur. Le zip suit le même nom que les applications : `dubSuite-0.2.10-windows-x64.zip`, et doit contenir `dubSuite.exe`.

## Dépôt privé

**Catalogue…** permet d'enregistrer un jeton GitHub. Il est écrit dans `%LOCALAPPDATA%\DubSuite\settings.json`. La variable `DUBSUITE_GITHUB_TOKEN` est lue en priorité. Laisser le champ vide conserve le jeton déjà enregistré ; cocher **Effacer le jeton enregistré** le retire.
