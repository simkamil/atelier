# Conteneur CFB : US-311

**État : US développée et validée le 2026-10-12 (lecture seule).** Revue indépendante et branchement au fuzzer (`tools/fuzz`) restent à faire à l'intégration.

`atelier-cfb` lit le format Compound File Binary ([MS-CFB], versions majeures 3 et 4) depuis une tranche d'octets. Aucune dépendance externe, `#![forbid(unsafe_code)]`, aucune API système (compilable en WASM). L'écriture de conteneurs, le contenu des flux (`.doc`, `.xls`), le déchiffrement et les flux de propriétés OLE sont hors périmètre ; le constructeur de conteneurs vit dans `crates/cfb/tests/common/mod.rs`.

```rust
use atelier_cfb::{is_cfb, Compound, Limits};

if is_cfb(&bytes) {
    let cfb = Compound::open(&bytes, Limits::default())?;
    for item in cfb.list() {
        println!("{} ({} octets)", item.path, item.entry.size());
    }
    let word = cfb.read("WordDocument")?;
    let part = cfb.read_range("1Table", 4096, 512)?; // lecture partielle
    let encrypted = cfb.exists("EncryptionInfo") && cfb.is_stream("EncryptedPackage");
}
# Ok::<(), atelier_cfb::CfbError>(())
```

## API

- `is_cfb(&[u8]) -> bool` : compare les huit octets de signature, sans analyse (distinction CFB / ZIP).
- `Compound::open(data, Limits) -> Result<Compound, CfbError>` ; `major_version()`, `sector_size()`, `anomalies()`.
- Arborescence : `root()`, `entry(id)`, `children(&Entry)`, `list()` (parcours en profondeur, enfants triés selon l'ordre CFB : longueur puis majuscules), `find(chemin)`.
- Tests d'existence : `exists`, `is_stream`, `is_storage` (le chemin vide désigne la racine).
- Métadonnées d'`Entry` : `id`, `name`, `kind` (`Root`, `Storage`, `Stream`), `clsid` (16 octets, ordre du fichier), `state_bits`, `created`/`modified` (FILETIME brut), `start_sector`, `size`, `child_ids`. `root_clsid()` est un raccourci.
- Lecture : `read(chemin)`, `read_range(chemin, offset, len)`, `read_entry`, `read_entry_range`. Le choix FAT / mini-FAT suit la taille déclarée (moins de 4096 octets : mini-flux). `len` est tronqué à la fin du flux ; `offset == taille` donne un tampon vide ; au-delà : `OutOfRange`. Seuls les secteurs de la plage sont copiés, mais toute la chaîne est validée à chaque lecture.
- Les chemins séparent les composants par `/` (composants vides ignorés). La comparaison des noms plie la casse par unité UTF-16 (mise en majuscules simple d'un caractère BMP, sinon unité inchangée), comme la règle du format ; un nom contenant `/` n'est donc pas adressable (et est refusé).

## Limites (`Limits`, toutes vérifiées avant allocation)

| Champ | Défaut | Contrôle |
|---|---|---|
| `max_file_size` | 1 Gio | taille de l'entrée, à l'ouverture |
| `max_entries` | 100 000 | emplacements du répertoire, pendant la lecture de la chaîne |
| `max_depth` | 64 | imbrication des stockages ; au-delà, le sous-arbre n'est pas indexé |
| `max_stream_size` | 256 Mio | taille déclarée d'un flux, avant toute lecture |
| `max_total_read` | 1 Gio | octets cumulés rendus par un même `Compound` |

Les tables (FAT, mini-FAT, répertoire) sont bornées par la taille du fichier : le nombre de secteurs FAT doit tenir dans le fichier, et la sortie d'une lecture n'est allouée qu'après validation complète de la chaîne, donc jamais plus grande que le fichier.

## Erreurs et anomalies

Erreurs fatales à l'ouverture (`CfbError`) : signature, version, ordre des octets, décalages de secteur incohérents avec la version, décalage du mini-secteur, seuil des mini-flux différent de 4096, fichier tronqué, nombre de secteurs FAT nul ou excédant le fichier, nombre de secteurs DIFAT inexact, secteur FAT ou DIFAT hors fichier / doublon / valeur spéciale, cycle ou fin manquante de la chaîne DIFAT, chaîne du répertoire invalide (cycle, hors fichier, vide), entrée racine invalide, dépassement de limite.

Erreurs de lecture, propres à un flux (les autres flux restent lisibles) : `ChainCycle` (détecté sans mémoire : une chaîne ne peut pas visiter plus de secteurs que le conteneur n'en a), `InvalidSector` (hors fichier, FREESECT, FATSECT, DIFSECT ou valeur réservée en milieu de chaîne), `ChainTooShort` / `ChainTooLong` (la chaîne doit avoir exactement ⌈taille / secteur⌉ éléments), `LimitExceeded`, `Truncated`. La mini-FAT et le mini-flux sont chargés à l'ouverture mais leur erreur est conservée : elle n'affecte que les flux de moins de 4096 octets. Un flux de taille nulle ne lit aucun secteur.

Anomalies non fatales (`Anomaly`, l'entrée ou le lien fautif est ignoré) : champs d'en-tête non conformes (CLSID, version mineure, réservé, remplissage v4, secteur final partiel, nom de la racine), entrée référencée plusieurs fois ou en cycle (l'arbre est parcouru de façon itérative avec un marquage par entrée, sans faire confiance à l'équilibre ni à l'ordre de l'arbre rouge-noir), lien hors du répertoire, nom invalide (longueur impaire, nulle ou supérieure à 64 octets, non terminé, NUL interne, `/`, substitut isolé), type d'entrée invalide ou seconde racine, couleur invalide, nom en double (la première entrée triée est conservée), profondeur dépassée, flux v3 de plus de 2 Gio. En v3, seuls les 32 bits de poids faible de la taille sont lus.

## Preuves

- `crates/cfb/tests/container.rs` : 42 tests (versions 3 et 4, mini-flux et seuil 4095/4096, flux vide, flux de 7,3 Mo exigeant plus de 109 secteurs FAT et un secteur DIFAT, chaînes DIFAT de 1 et 2 secteurs en v3 et v4, arborescence de 40 niveaux, noms en casse différente / non ASCII / hors BMP, arbre dégénéré et non trié, lectures partielles, détection, un test par type de corruption : cycles FAT, mini-FAT, DIFAT, répertoire et arbre, secteurs hors fichier, chaînes trop courtes ou trop longues, tailles absurdes, doubles références, noms, types, couleurs, valeurs spéciales mal placées, troncatures, limites).
- `crates/cfb/tests/mutations.rs` : 6 tests, dont 15 900 mutations à graine fixe sur cinq conteneurs (octets, mots de 32 bits à valeurs limites, remises à zéro, copies d'entrées de 128 octets, troncatures), toutes les troncatures de petits conteneurs et 2 000 fichiers aléatoires à signature valide ; chaque itération ouvre, liste et lit tous les flux, avec un plafond de 5 s par itération. Le test vérifie aussi que la campagne atteint les chemins d'acceptation et de rejet.
- `crates/cfb/tests/real_files.rs` : contrôle local optionnel (variable `ATELIER_CFB_SAMPLES`, répertoires séparés par `;`), aucun fichier copié. Résultat du 2026-10-12 : 399 fichiers `.msi` / `.msg` / `.doc` essayés, 153 non CFB (`.msg` texte), 246 conteneurs ouverts (233 `.msi`, 13 `.doc`), 13 923 flux lus en entier sans erreur ni anomalie, 5,0 Go lus.
- Point d'entrée pour le fuzzer : `Compound::open(data, Limits)` puis `list()` et `read_entry` sur chaque flux (voir `exercise` dans `mutations.rs`).
