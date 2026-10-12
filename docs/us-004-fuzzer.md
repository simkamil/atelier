# US-004 — Fuzzer maison

`tools/fuzz` (paquet `atelier-fuzz`) est un fuzzer mutationnel écrit sans
dépendance externe, pour les analyseurs qui lisent des données non fiables
(SPEC §6.13). Il ne contient aucune instrumentation de couverture : il tire
des entrées, les exécute sur un fil surveillé et signale panique, blocage ou
dépassement du budget de sortie.

## Usage

```powershell
cargo run --release -p atelier-fuzz --locked --offline -- list
cargo run --release -p atelier-fuzz --locked --offline -- run xml --duration 60 --seed 7
cargo run --release -p atelier-fuzz --locked --offline -- run --all --iterations 50000 --minimize
cargo run --release -p atelier-fuzz --locked --offline -- replay png tools/fuzz/crashes/png/panic-0123456789abcdef.bin
cargo run --release -p atelier-fuzz --locked --offline -- minimize png tools/fuzz/crashes/png/panic-0123456789abcdef.bin
cargo run --release -p atelier-fuzz --locked --offline -- seed
```

| Commande | Effet |
|---|---|
| `list` | Affiche les cibles. |
| `run CIBLE` ou `run --all` | Fuzze une cible, ou toutes à la suite. `--duration S` et `--iterations N` s'appliquent à **chaque** cible ; sans l'un ni l'autre, 10 000 exécutions. |
| `replay CIBLE FICHIER...` | Exécute chaque fichier une fois et affiche le résultat ou la défaillance. |
| `minimize CIBLE FICHIER [--output F]` | Réduit une entrée fautive (sortie par défaut : `FICHIER.min`). |
| `seed [CIBLE]` | Écrit les graines intégrées dans `corpus/<cible>/`. |

Options de `run` : `--seed N` (décimal ou `0x…`, défaut 1), `--timeout-ms N`
(échéance par entrée, défaut 5000), `--max-len OCTETS` (taille maximale d'une
entrée mutée, défaut 65 536), `--max-output-mib N` (budget mémoire, défaut 256),
`--minimize` (ajoute une copie minimisée de chaque nouvelle défaillance),
`--inflight` (réécrit `crashes/inflight.bin` avant chaque exécution, voir les
limites), `--root DIR` (défaut : le dossier `tools/fuzz`).

Codes de sortie : 0 aucune défaillance ; 1 erreur d'usage ou d'entrée/sortie ;
2 défaillance trouvée (panique ou budget de sortie dépassé) ; 3 blocage trouvé.
Chaque cible imprime une ligne de résumé : graine, itérations, durée, entrées
par seconde, taille du corpus de travail et nombre de défaillances distinctes
(`--all` ajoute un total).

## Reproductibilité

Toute l'aléa vient d'un générateur SplitMix64 initialisé par `--seed`. Pour une
cible, une graine et un corpus donnés, la suite des entrées exécutées est
identique d'une exécution à l'autre (test `same_seed_generates_the_same_inputs`) :
le corpus de travail ne dépend que des résultats des cibles, qui sont
déterministes. Une limite de durée change seulement le nombre d'itérations
atteint, pas la suite. Le fichier `.txt` de chaque défaillance enregistre la
graine, le numéro d'itération et les mutations appliquées.

## Fonctionnement

* **Corpus.** Au démarrage : graines intégrées de la cible, puis fichiers de
  `corpus/<cible>/` triés par nom, sans doublon. Les graines sont de petites
  copies versionnées (`seed` les régénère) de fixtures du dépôt
  (`crates/*/tests`) ou d'entrées construites par les encodeurs des crates ;
  le tout pèse environ 150 Kio. Rien n'est téléchargé.
* **Mutations.** Inversion de 1 à 3 bits ; remplacement d'un champ de 1, 2, 4
  ou 8 octets par une valeur limite (0, 1, bornes signées et non signées, en
  petit et grand boutiste) ; insertion, suppression, duplication de blocs ;
  épissure entre deux entrées du corpus ; insertion ou écrasement par un jeton
  du dictionnaire ; troncature. Un à cinq empilements par entrée.
* **Signal de substitution à la couverture.** Chaque cible renvoie un `Outcome` :
  une classe (acceptée, ou variante d'erreur avec sa raison statique, chiffres
  retirés) et la taille de sortie produite. Une entrée rejoint le corpus de
  travail (2 048 entrées au plus) si le couple (classe, logarithme base 2 de la
  taille de sortie) est nouveau.
* **Panique.** `catch_unwind` sur le fil de travail ; un crochet de panique
  enregistre message et `fichier:ligne:colonne`. Les défaillances sont
  dédupliquées par type et emplacement de panique.
* **Blocage.** Chaque entrée s'exécute sur un fil de travail ; l'outil attend
  `--timeout-ms`. Au dépassement, l'entrée est sauvegardée et la campagne
  s'arrête avec le code 3. Le fil bloqué ne peut pas être tué : il est
  abandonné (le processus se termine juste après).
* **Mémoire.** Il n'y a pas d'allocateur global (`unsafe` interdit) : la
  mémoire réellement consommée n'est **pas mesurée**. Le contrôle est la
  taille de sortie que la cible rapporte (pixels décodés, octets décompressés…)
  comparée à `--max-output-mib`. Un analyseur qui alloue beaucoup sans le
  rapporter n'est pas détecté.
* **Sauvegarde.** `crashes/<cible>/<type>-<empreinte FNV-1a>.bin`, avec un
  `.txt` (cible, type, graine, itération, emplacement, message, mutations) et,
  avec `--minimize`, un `.min.bin`. Le dossier est ignoré par git, sauf
  `.gitkeep`, par son propre `.gitignore`.
* **Minimisation.** Suppression de blocs de taille décroissante (moitié,
  quart… un octet), passes répétées tant que la **même** défaillance (type et
  emplacement de panique) se reproduit ; plafonds de 20 000 exécutions et de
  60 s. Les entrées minimisées d'un vrai défaut sont à conserver dans
  `regressions/<cible>/` avec une note.

## Cibles

Chaque cible applique les limites par défaut de son analyseur.

| Cible | Analyseur |
|---|---|
| `inflate`, `zlib` | `atelier-compress::InflateState`, sortie bornée à 16 Mio (l'API n'a pas de défaut) |
| `zip` | `atelier-zip::Archive::open` puis lecture de 64 membres au plus |
| `xml` | `atelier-xml::Reader` : lecture stricte, tolérante, et capture d'un sous-arbre opaque |
| `png`, `gif`, `bmp`, `jpeg` | décodeurs `atelier-image` |
| `opentype` | `FontFace` (faces 0 et 1) : tables, cmap, métriques, GSUB/GPOS |
| `truetype` | contours `glyf` de `FontFace` |
| `cff` | `atelier-font::cff::CffFont` : charset, polices CID, contours |
| `opc` | `atelier-fmt-ooxml::Package::read` puis `write` |
| `encodings` | `atelier-unicode` : détection, décodage et réencodage des sept encodages |
| `display-list` | `atelier-render::DisplayList::decode`, revalidation et réencodage |
| `recovery-journal` | `atelier-model::journal::replay` (entrée = longueur u16, instantané, journal) |

Absentes volontairement : `core-wasm` et `desktop-win` (consigne du lot). Le
décodeur du protocole binaire (US-091) vit dans `core-wasm/src/protocol.rs` :
il n'est donc pas accessible depuis un crate non FFI et n'a pas de cible. Le
conteneur CFB n'existe pas encore sur cette branche.

## Ajouter une cible

1. Écrire `fn run(input: &[u8]) -> Outcome` qui appelle l'analyseur avec ses
   limites par défaut. Renvoyer `Outcome::accepted(taille_de_sortie)` en cas de
   succès, `Outcome::rejected(&erreur)` sinon, ou `Outcome::labelled(...)` pour
   une classe libre. Ne pas attraper les paniques : c'est le travail du fuzzer.
2. Facultatif : un `&[&[u8]]` de jetons (signatures, balises, marqueurs) et une
   `fn seeds() -> Vec<Vec<u8>>` de petites entrées valides.
3. Ajouter une ligne dans `registry()` (`tools/fuzz/src/targets.rs`) :

```rust
Arc::new(FnTarget::new("cfb", cfb).dictionary(CFB_TOKENS).seeds(cfb_seeds)),
```

Une cible plus complexe peut implémenter directement le trait `FuzzTarget`
(`name`, `run`, `dictionary`, `seeds`). Le conteneur CFB (autre branche) se
branchera ainsi : fonction `cfb(input)` appelant l'ouverture du conteneur et la
lecture de ses flux, graines issues de ses fixtures, puis la commande
`seed cfb` pour matérialiser le corpus.

## Limites par rapport à un fuzzer guidé par la couverture

* Pas d'instrumentation : le fuzzer ne sait pas quelles branches une entrée a
  exercées. Le signal (classe d'erreur, taille de sortie) est grossier ; les
  corpus de travail restent petits (9 à 100 entrées en 60 s) et le fuzzer
  explore mal les chemins profonds derrière plusieurs validations
  (sommes de contrôle, tailles imbriquées), sauf si une graine les passe déjà.
* Pas de récupération d'un processus tué : un dépassement de pile ou un échec
  d'allocation interrompt le processus sans passer par `catch_unwind`.
  `--inflight` réécrit l'entrée en cours dans `crashes/inflight.bin` avant
  chaque exécution (coût d'une écriture de fichier) pour la retrouver ; le
  rejeu depuis (cible, graine, corpus) reproduit aussi le plantage.
  Les fils de travail ont 64 Mio de pile.
* Mémoire non mesurée (voir plus haut) ; un blocage dans une section qui
  n'appelle pas la cible (allocation géante) ne peut être détecté que par
  l'échéance.
* Débit limité par l'aller-retour vers le fil surveillé (environ 2 000 à 3 000
  entrées/s en release sur la machine de mesure), même pour des entrées de
  quelques dizaines d'octets.
* Les entrées ne dépassent pas 64 Kio par défaut ; les défauts qui
  demandent de très gros fichiers ne sont pas atteints.
* Pas de parallélisme : une cible à la fois.

## Campagne

Machine : Windows 11, build `--release`, une cible à la fois, graine
20261012, 60 s par cible, `--minimize --inflight --timeout-ms 5000`.

| Cible | Itérations | Entrées/s | Corpus final | Défaillances |
|---|---:|---:|---:|---:|
| inflate | 133 682 | 2 226 | 39 | 0 |
| zlib | 137 066 | 2 284 | 26 | 0 |
| zip | 146 991 | 2 450 | 17 | 0 |
| xml | 140 716 | 2 270 | 100 | 0 |
| png | 172 195 | 2 775 | 15 | 0 |
| gif | 138 278 | 2 305 | 45 | 0 |
| bmp | 131 406 | 2 187 | 44 | 0 |
| jpeg | 176 660 | 2 849 | 59 | 0 |
| opentype | 172 575 | 2 876 | 63 | 0 |
| truetype | 172 125 | 2 860 | 66 | 0 |
| cff | 191 057 | 3 184 | 25 | 0 |
| opc | 165 449 | 2 757 | 15 | 0 |
| encodings | 149 819 | 2 497 | 67 | 0 |
| display-list | 175 258 | 2 920 | 9 | 0 |
| recovery-journal | 174 788 | 2 913 | 20 | 0 |

Total : environ 2,38 millions d'exécutions, aucune panique, aucun blocage,
aucun dépassement du budget de sortie. `tools/fuzz/regressions/` ne contient
donc que `.gitkeep`. Ce résultat n'est pas une preuve d'absence de défaut : voir
les limites. La capacité de détection de l'outil est établie par ses propres
tests (`finds_saves_deduplicates_and_minimizes_a_panic`,
`detects_a_hang_without_blocking_and_stops_the_run`,
`reported_output_over_budget_is_a_memory_failure`), pas par les analyseurs.

Les tests de l'outil exécutent en plus 400 itérations par cible
(`short_campaign_over_every_target_finds_nothing`) ; une régression d'un
analyseur y apparaîtrait comme un échec de test.
