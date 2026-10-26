# dubSuite

Lanceur des applications dub. Il affiche les programmes du catalogue, installe celui qui est choisi, l'ouvre, et le remplace quand une version plus récente est publiée sur GitHub.

Le code vit dans `M:\0-CRDD\Rust\dubSuite`, à côté de `dubDSR` et de `dubFocus`. Chaque application reste son propre dépôt.

```powershell
cargo run
```

Les programmes installés sont rangés dans `%LOCALAPPDATA%\DubSuite\apps\`.

## Ajouter une application

Une entrée dans `catalog.json` suffit. Le lanceur relit ce fichier au démarrage.

```json
{
  "id": "dubphoto",
  "name": "dubPhoto",
  "mark": "Ph",
  "summary": "Courte description affichée sur la carte",
  "repo": "DarthDub66/dubPhoto",
  "exe": "dubPhoto.exe",
  "accent": "#7dcea0"
}
```

`id` sert de nom de dossier. Lettres, chiffres, tiret et souligné seulement.

Pour que la carte propose **Installer**, le dépôt doit avoir une Release GitHub dont le `.zip` contient le `.exe` à la racine. Le nom du zip se termine par la plateforme :

`dubPhoto-0.1.0-windows-x64.zip`

S'il n'y a qu'un seul zip dans la Release, il est utilisé même sans ce suffixe. Un `.exe` seul, comme `dubdsr1.2.1.exe`, est aussi accepté. Un fichier `checksums.txt` au format `sha256sum`, joint à la même Release, est vérifié avant l'installation.

```powershell
gh release create v0.1.0 Distribution/dubPhoto-0.1.0-windows-x64.zip --repo DarthDub66/dubPhoto --title "dubPhoto 0.1.0" --generate-notes
```

Le tag doit être la version de `Cargo.toml`, précédée de `v`.

## Catalogue en ligne

Tant que l'adresse est vide, le lanceur utilise le `catalog.json` du projet, puis celui inclus dans le programme. Pour qu'une nouvelle application apparaisse sans recompiler le lanceur, publiez ce fichier et indiquez son adresse dans **Catalogue…** :

`https://raw.githubusercontent.com/ORGANISATION/dubSuite/main/catalog.json`

La variable `DUBSUITE_CATALOG_URL` a le même effet.

## Mise à jour du lanceur

Dans `catalog.json`, renseignez le dépôt du lanceur :

```json
"launcher": { "repo": "ORGANISATION/dubSuite", "exe": "dubSuite.exe" }
```

Quand une Release plus récente existe, une bande l'annonce. Le zip suit le même nom : `dubSuite-0.2.0-windows-x64.zip`.

## Dépôt privé

**Catalogue…** permet d'enregistrer un jeton GitHub. Il est écrit dans `%LOCALAPPDATA%\DubSuite\settings.json`. La variable `DUBSUITE_GITHUB_TOKEN` est lue en priorité.
