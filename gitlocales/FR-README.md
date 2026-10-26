[🇷🇺 Русский](RU-README.md) · [🇬🇧 English](EN-README.md) · [🇪🇸 Español](ES-README.md) · [🇩🇪 Deutsch](DE-README.md) · [🇫🇷 Français](FR-README.md) · [🇨🇳 中文](ZH-README.md)

---

# PlicZip

Un archiveur avec son propre format `.plz` : index en fin de fichier, compression deflate/brotli, BLAKE3 sur chaque fichier et chaque bloc, chiffrement par mot de passe, archives multi-volumes, extraction sécurisée. Écrit en Rust, sans dépendances C.

```text
$ pliczip a backup.plz ./documents -x9
$ pliczip a secret.plz ./documents --password=secret --volume-size 700M
$ pliczip l backup.plz -l
$ pliczip t backup.plz
$ pliczip x backup.plz -o ./restauré
```

> Les chemins des exemples sont des espaces réservés — remplacez-les par vos propres répertoires. Si un chemin
> n'existe pas, PlicZip le dit clairement : `pliczip: error: chemin introuvable : ".\photos"`.

## Compilation

```
cargo build --release
# CLI:  target\release\pliczip.exe (2.7 Mo)
# GUI:  target\release\pliczip-gui.exe (14.6 Mo)
```

Rust pur uniquement : `flate2` (miniz_oxide), `brotli`, `chacha20poly1305`, `argon2` — pas de code C. C'est pourquoi la compilation ne nécessite pas MSVC `cl.exe` et fonctionne dans des chemins contenant des caractères cyrilliques. La GUI utilise egui/eframe, également sans dépendances C.

## Commandes

| Commande | Alias | Ce qu'elle fait |
|---|---|---|
| `add <archive> <chemins...>` | `a` | empaqueter fichiers et répertoires |
| `extract <archive>` | `x` | extraire |
| `list <archive>` | `l` | afficher le contenu |
| `test <archive>` | `t` | vérifier l'intégrité sans rien écrire |
| `info <archive>` | `i` | informations sur l'archive |

### Empaquetage

```
pliczip a backup.plz ./documents ./notes     # vos propres répertoires, pas des espaces réservés
pliczip a -x9 backup.plz ./sources           # compression maximale
pliczip a --solid backup.plz ./sources       # coller les petits fichiers
pliczip a -x0 backup.plz ./video             # sans compression, juste un conteneur
pliczip a -c "avant les vacances" backup.plz .  # avec un commentaire
pliczip a backup.plz src --exclude "**/*.tmp" --exclude "target"
pliczip a src.plz src --include "**/*.rs"    # seulement les fichiers nécessaires
```

Un répertoire est stocké sous son propre nom : `a x.plz photos` produit `photos/...` à l'intérieur, comme avec 7-Zip.

### Niveaux de compression

| Niveau | Méthode | Quand l'utiliser |
|---|---|---|
| `-x0` | sans compression | déjà compressé (vidéo, jpg, autres archives) |
| `-x1..4` | deflate | rapide, modéré |
| `-x5` (par défaut) | brotli q5 | équilibre raisonnable |
| `-x9..11` | brotli q9..q11 | lent, mais texte/code/logs compressent nettement mieux |

Si la compression ne réduit pas un bloc, il est stocké tel quel — l'archive ne gonfle pas.

### Filtres

`--exclude` et `--include` acceptent des masques et peuvent être répétés :

- `*` — n'importe quel caractère sauf `/` ;
- `?` — exactement un caractère sauf `/` ;
- `**` — n'importe quel caractère, y compris `/`.

L'exclusion l'emporte toujours sur l'inclusion. Un répertoire exclu n'est pas parcouru du tout : `--exclude target` sur un gros arbre fait gagner du temps, pas seulement de l'espace.

### Progression

L'empaquetage, l'extraction et la vérification affichent la progression sur une seule ligne de stderr :

```
 compression 812.4 MiB / 1.42 GiB  57%  186 MiB/s  restant 4.3s
```

La vitesse et le temps restant sont calculés sur une fenêtre glissante de cinq secondes, pas sur la moyenne totale : sur des données mixtes, la moyenne trompe d'un ordre de grandeur — les premières secondes servent à l'indexation et aux petits fichiers, puis viennent les gigaoctets. `-q` désactive la progression.

## Protection par mot de passe

```
pliczip a secret.plz ./documents -p                  # demande le mot de passe deux fois
pliczip a secret.plz ./documents --password="mot de passe long"
pliczip x secret.plz -o ./out --password="mot de passe long"
pliczip t secret.plz            # demande le mot de passe en interactif
```

- `-p` sans valeur demande interactivement — le mot de passe ne finit ni dans
l'historique du shell ni dans la liste des processus ;
- `--password=…` uniquement avec le signe égal — sinon `-p backup.plz` avalerait
le nom de l'archive comme mot de passe ;
- `--password-file pw.txt` — le mot de passe est lu dans la première ligne du fichier ;
- `PLICZIP_PASSWORD` — variable d'environnement, pratique pour les scripts.
- la clé est dérivée via **Argon2id** (64 MiB, 3 passes) à partir d'un sel aléatoire ;
- les données et les noms de fichiers sont chiffrés avec **ChaCha20-Poly1305** par morceaux de 1 MiB ;
- l'index est aussi chiffré, donc même la liste des fichiers est invisible sans le mot de passe ;
- un mauvais mot de passe et une archive corrompue sont indistinguables par le message — pas d'oracle ;
- le mot de passe n'est pas récupérable. Perdu — archive perdue.

Le découpage importe : un fichier de 20 Go est chiffré et déchiffré en flux, sans occuper 20 Go de RAM.

## Volumes

```
pliczip a backup.plz ./photos --volume-size 700M   # produit backup.plz.001, backup.plz.002, …
pliczip x backup.plz -o ./out      # lit tous les volumes à la suite
```

Les volumes sont un simple découpage d'octets ; le format interne ne change pas. Le lecteur n'a pas besoin de connaître la taille des volumes — il concatène les fichiers dans l'ordre. Indiquer `.001` suffit ; le reste est trouvé automatiquement.

Si un volume intermédiaire est perdu, l'archive ne se décompresse pas : une erreur explicite vaut mieux qu'un résultat silencieusement incomplet.

## Extraction

```
pliczip x backup.plz                     # dans le répertoire courant
pliczip x backup.plz -o ./restauré
pliczip x backup.plz --skip-existing     # ne pas toucher aux fichiers existants
pliczip x backup.plz --no-verify         # sans vérification des hachages (plus rapide)
```

Par défaut, les sommes de contrôle sont vérifiées et l'extraction s'arrête avec une erreur en cas de non-concordance. Les heures de modification et les permissions unix sont restaurées (sous Windows, l'attribut lecture seule).

## Sortie machine

`l`, `i` et `t` comprennent `--json` :

```
pliczip l backup.plz --json
pliczip i backup.plz --json
pliczip t backup.plz --json     # {"ok": true, ...} ou {"ok": false, "error": ...}
```

## Interface graphique

```
pliczip-gui                    # fenêtre vide
pliczip-gui backup.plz         # ouvrir une archive (c'est ainsi que fonctionne le double-clic)
```

Fenêtre egui/eframe, binaire séparé dans le même workspace, partage la bibliothèque avec la CLI.

```
┌───────────────────────────────────────────────────────────┐
│ Fichier  Affichage  Aide                                  │
├───────────────────────────────────────────────────────────┤
│ + Ajouter │ Extraire │ Vérifier │ Modifier   [recherche]  │
├──────────────────────────────────────────┬────────────────┤
│ NOM         TAILLE DANS ARCHIVE MÉTHODE …│ RÉSUMÉ         │
│ data/                                 —  │ Format  PLZ2   │
│   docs/readme.md        1.6 KiB  28 B brotli │ VOLUME      │
│   photo/IMG_0001.jpg    7.3 KiB  38 B brotli │ Avant       │
│                                          │ Économie 99%   │
│                                          │ ┌───────────┐  │
│                                          │ │✓ Intégrité │  │
│                                          │ │ confirmée  │  │
│                                          │ └───────────┘  │
├──────────────────────────────────────────┴────────────────┤
│ Terminé · 3 blocs · brotli/store · normal  8.9 KiB → 85 B │
└───────────────────────────────────────────────────────────┘
```

Ce qu'elle sait faire :

- **ouverture** — via le menu, double-clic sur `.plz` (chemin en argument) ou
glisser-déposer dans la fenêtre ; une archive chiffrée demande le mot de passe ;
- **table de contenu** — noms, tailles, part dans l'archive, méthode, date ;
recherche par nom ;
- **résumé et badge d'intégrité** — vert « confirmée », rouge
« corrompue », gris « non vérifiée » : l'état est toujours visible, pas
seulement à la demande ;
- **création d'archive** — glisser des fichiers, curseur « rapide / équilibré /
maximum » avec explication des conséquences, solid, mot de passe, taille de volume,
commentaire ;
- **extraction** — choix du répertoire, vérification des hachages par défaut ;
- **progression avec prévision honnête** — vitesse et temps restant sont
calculés sur les dernières secondes, pas moyennés depuis le début ;
- **paramètres** — thème (sombre/clair) et accent (laiton/acier/vert)
enregistrés dans `%APPDATA%/pliczip/config.json`.

Pas encore fait : modification d'une archive existante (bouton « Modifier »), association `.plz` avec l'Explorateur et menu contextuel — c'est le prochain bloc.

## Intégrité

Quatre niveaux, tous sur BLAKE3 :

```
fichier ──hash──┐
                ├── bloc ──hash──┐
autres fichiers ┘                ├── index ──hash──┐
                                 ┘                ├── pied
longueur de l'index ──────────────────────────────┘
```

- **fichier** — détecte la corruption du contenu même si un bloc s'est décompressé ;
- **bloc** — détecte la corruption du flux compressé entier ;
- **index** — protège la liste des fichiers : sans lui, des fichiers pourraient être
retirés de l'archive et elle resterait « valide » ;
- **signature au début et à la fin** — détecte immédiatement un fichier tronqué, avant l'analyse.

Dans une archive chiffrée, une étiquette AEAD est ajoutée sur chaque morceau, et les hachages de blocs sont calculés sur les données déchiffrées — la vérification fonctionne donc aussi là.

`pliczip t` vérifie tout sans créer un seul fichier. Le code de retour `1` signifie corruption, `2` — autres erreurs.

## Sécurité de l'extraction

| Attaque | Défense |
|---|---|
| `../../secret` (zip-slip) | politique de chemins : `..`, `.`, segments vides interdits dans le format |
| chemins absolus `/etc/…`, `C:\…` | interdits lors de la validation de l'index |
| ADS sur NTFS (`file.txt:hidden`) | le caractère `:` est interdit |
| noms réservés (`CON`, `NUL`, `aux.txt`) | interdits |
| `Report.txt` + `report.txt` | unicité insensible à la casse |
| lien `docs -> /etc` et écriture à travers | les liens sont créés en dernier, jamais vers l'extérieur |
| répertoire-lien pré-planté | le chemin est vérifié après résolution du parent |
| « bombe » de gigaoctets | le volume d'écriture est borné par l'index, plus une limite de taux de compression |

La validation des chemins se fait deux fois : à l'écriture (pour ne pas créer de déchets) et à la lecture (pour ne pas décompresser des déchets étrangers).

## Mesures

126 MiB de données mixtes (76 MiB de texte + 50 MiB incompressibles), cette machine, `--release` :

| Mode | Archive | De l'original | Vitesse d'empaquetage |
|---|---|---|---|
| `-x1` deflate | 50.7 MiB | 40.2 % | 227 MiB/s |
| `-x5` brotli | 50.0 MiB | 39.6 % | 185 MiB/s |
| `-x9` brotli | 50.0 MiB | 39.6 % | 58 MiB/s |

2000 fichiers de 8 Ko (33 MiB) — c'est là que le mode solid décide :

| Mode | Archive | Compression | Temps |
|---|---|---|---|
| normal `-x5` | 0.72 MiB | 45× | 1.48 s |
| `--solid -x5` | 0.32 MiB | 101× | 0.45 s |
| `--solid -x9` | 0.32 MiB | 103× | 1.99 s |

L'extraction est d'environ 700 MiB/s ; la vérification d'intégrité est à peu près à la même vitesse.

Le chiffrement ajoute 16 octets par mégaoctet de données et environ 0.5 s pour la dérivation de la clé à partir du mot de passe (une fois par archive).

## Format

Disposition octet par octet : documentation du module [`src/format.rs`](../src/format.rs).

```
"PLZ1" | blocs de données | index (CBOR) | longueur de l'index u64 | BLAKE3 de l'index | "PLZ1"
"PLZE" | paramètres KDF | blocs de données | index | pied          (chiffré)
```

Index en fin de fichier — comme dans 7z : cela permet d'écrire l'archive en flux sans connaître à l'avance les tailles compressées. Le prix : la lecture nécessite `seek`, donc l'archive ne se lit pas encore depuis un tube.

Le lecteur de la version 2 ouvre aussi les archives de la version 1 — le format est rétrocompatible.

## Design

Principes en bref : l'éclair comme métaphore du nom, le laiton comme seul accent, le vert et le rouge réservés à l'intégrité, et un badge permanent « Intégrité confirmée » dans la fenêtre principale — parce que c'est la seule chose qui distingue essentiellement PlicZip de 7-Zip.

## Ce qui manque encore

Une liste honnête, pour ne pas chercher :

- **recovery record** — un secteur endommagé n'est pas récupérable (prévu ; c'est la
différence avec 7z et zip) ;
- **déduplication** — les fichiers identiques sont encore stockés séparément, bien que les
hachages existent déjà pour cela ;
- **lecture depuis stdin** — à cause de l'index en fin de fichier ;
- **noms non-UTF-8** — un chemin non représentable en UTF-8 n'est pas archivé ;
- **modification d'archive** — les fichiers ne peuvent être ni ajoutés à une archive
terminée ni supprimés (le bouton « Modifier » dans la GUI est vide pour l'instant) ;
- **intégration à l'Explorateur** — l'association `.plz` et le menu contextuel n'existent pas encore ;
- **signature numérique** de l'archive ;
- **chiffrement des noms seuls** sans chiffrer les données — volontairement non fait : crée
une fausse impression de protection.

## Architecture

```
src/
  format.rs    conteneur : signatures, structures d'index, pied, validation
  crypto.rs    Argon2id, ChaCha20-Poly1305, chiffreur et déchiffreur en flux
  compress.rs  deflate/brotli, sélection de méthode, lecteur hachant
  safety.rs    politique de chemins : normalisation, zip-slip, noms Windows
  glob.rs      masques --exclude / --include
  source.rs    source d'octets : un seul fichier ou un ensemble de volumes
  writer.rs    parcours du FS, planification des blocs, compression parallèle, volumes
  reader.rs    ouverture, liste, extraction, vérification d'intégrité
  util.rs      tailles, progression avec ETA, pluralisation, UTF-8 dans la console Windows
  main.rs      CLI sur clap
gui/
  main.rs      point d'entrée, lancement de fenêtre
  app.rs       état : archive ouverte, dialogues, opérations en arrière-plan
  views.rs     fenêtre principale : menu, barre d'outils, table, résumé, barre d'état
  dialogs.rs   création, extraction, mot de passe, erreur, progression
  theme.rs     palette, accents
  config.rs    paramètres en JSON
  icon.rs      signe de l'application, assemblé en code
  progress.rs  canal de progression et estimation du temps restant
tests/
  cli.rs       tests de bout en bout via de vrais processus
  security.rs  archives malveillantes assemblées à la main
legacy/        prototype du format YZ01 dont le projet a grandi
```

La progression passe par `ProgressSink` : la bibliothèque rapporte, la CLI imprime une ligne sur stderr, la GUI remplit une barre et estime le temps restant. C'est pourquoi la fenêtre ne se bloque pas pendant l'empaquetage.

La mémoire est bornée : les blocs jusqu'à 32 MiB sont assemblés en mémoire et compressés en parallèle (rayon) ; tout ce qui est plus gros est compressé en flux directement dans le fichier. Idem pour le chiffrement : morceaux de 1 MiB. Une archive de dizaines de gigaoctets est empaquetée et extraite sans occuper autant de mémoire.

## Développement

```
cargo test --workspace   # 104 tests : unitaires, CLI, sécurité, GUI, doctest
cargo build --release
```

La partie bibliothèque est disponible séparément :

```
use pliczip::prelude::*;

create(&archive, &[input], &CreateOptions::default())?;
let mut ar = Archive::open_with(&archive, Some("mot de passe"))?;
ar.extract(&ExtractOptions { output: dir, ..Default::default() })?;
```

Licence : [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).

