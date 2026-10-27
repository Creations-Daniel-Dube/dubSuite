# dubSuite

Lanceur des applications dub. Il affiche le catalogue, installe le programme choisi, l'ouvre, le met à jour quand une version plus récente est publiée, et peut le désinstaller.

```powershell
cargo run
```

La version du lanceur est affichée en haut de la fenêtre.

## Utilisation

Chaque carte montre le nom, la description, la dernière version publiée et, le cas échéant, la version déjà installée.

- **Installer** télécharge la version, vérifie la somme SHA-256 quand elle est jointe, puis place les fichiers dans le dossier de l'application.
- **Ouvrir** lance le programme installé.
- **Mettre à jour** apparaît quand la version publiée est plus récente que la copie locale. Si la copie locale est plus récente, la carte le signale et ne propose pas de revenir en arrière.
- **Désinstaller** demande confirmation, puis efface le dossier de l'application, son ancienne copie et les paquets mis en cache pour elle.
- Le champ de recherche filtre par nom, description ou identifiant.
- **Actualiser** relit le catalogue et interroge de nouveau GitHub. Un seul téléchargement à la fois ; **Annuler** l'interrompt.

Les programmes installés sont rangés dans `%LOCALAPPDATA%\DubSuite\apps\`. Sous Linux, le dossier est `$XDG_DATA_HOME/dubsuite`, ou `~/.local/share/dubsuite`.

Quand une version plus récente du lanceur existe, une bande l'annonce. **Mettre à jour** remplace le programme, puis le relance.
