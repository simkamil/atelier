# US-300 — Paquets OPC

`atelier-fmt-ooxml` traite le conteneur OPC commun à DOCX, DOTX et DOCM. Il
charge les parties sans interpréter `word/document.xml`, les styles ou le
contenu Word. Chaque partie exposée contient une URI `PartUri`, son type MIME
OPC et ses octets d'origine.

```rust
use atelier_fmt_ooxml::{Package, PartUri, TargetMode};

let package = Package::read(&docx_bytes)?;
let document = package.part(&PartUri::parse("word/document.xml")?)?;
let relationships = package.relationships(Some(&document.uri));
let output = package.write()?;
```

`Package::read` valide l'index ZIP, les noms de membres, `[Content_Types].xml`,
les fichiers `.rels`, les URI internes et la présence des cibles. Les relations
externes conservent leur cible textuelle et leur mode ; aucun accès réseau n'est
effectué. Les chemins relatifs sont résolus depuis la partie propriétaire, y
compris les segments `..` qui restent dans la racine du paquet. Les URI sortant
de cette racine, les doublons, les relations mal formées et les parties sans
type MIME entraînent une erreur `OpcError`.

Les noms de parties sont comparés sans tenir compte de la casse ASCII ; deux
noms équivalents ou dont l'un est un préfixe de chemin de l'autre sont refusés.
Les noms de métadonnées `.rels` sont réservés au conteneur. Les références
internes résolvent leur chemin uniquement : query et fragment restent dans la
cible conservée, et une cible limitée à un query ou un fragment désigne sa
partie source. Les types MIME suivent la syntaxe media type et paramètres HTTP,
et les extensions de `[Content_Types].xml` suivent le schéma OPC.

Les parties inconnues restent opaques : leurs octets et types MIME sont
conservés à l'écriture. Les relations sont conservées avec leur identifiant,
leur type, leur cible et leur mode. Le writer trie les parties et relations et
utilise les horodatages ZIP fixes de `atelier-zip`, ce qui rend la sortie
déterministe pour un même paquet.

Les propriétés core exposent titre, sujet, auteur, mots-clés, description,
auteur de dernière modification, révision, dates, catégorie, état, identifiant,
langue et version. Les propriétés app exposent l'application, sa version, la
société, le responsable et les statistiques courantes. Les enfants XML
inconnus de `core.xml` et `app.xml` sont capturés comme sous-arbres XML validés
et réémis à l'écriture. Les propriétés mutables sont disponibles via
`core_properties_mut()` et `app_properties_mut()`. Si un paquet neuf n'a pas
encore ces parties, `write()` crée `docProps/core.xml` ou `docProps/app.xml`
avec leur relation OPC racine. Une partie déjà présente sous le nom requis
provoque une erreur `Duplicate` afin de ne pas écraser ses octets.

`Package::read_with_limits` permet de réduire les plafonds par rapport aux
valeurs par défaut : limites ZIP, mémoire XML, taille d'une partie XML, octets
totaux, nombre de parties et nombre de relations. Les limites ZIP par défaut
sont de 10 000 entrées, 256 MiB par entrée, 1 GiB au total et un ratio de
compression maximal de 200. Une erreur de limite est retournée avant de
construire le paquet complet.

```rust
use atelier_fmt_ooxml::{Limits, Package};

let mut limits = Limits::default();
limits.max_xml_part_bytes = 2 * 1024 * 1024;
limits.max_total_part_bytes = 128 * 1024 * 1024;
let package = Package::read_with_limits(&docx_bytes, limits)?;
```

La couche OPC ne fait pas l'import sémantique WordprocessingML. Cette étape
relève de US-301 (texte, propriétés et styles). La numérotation correspond à US-302.

## Validation d’intégration — 2026-10-12

La [passe finale](../_bmad-output/implementation-artifacts/styles-formats-raster-validation.json)
réussit 41 contrôles, 507 tests workspace et 55 tests natifs release,
avec sources et lock inchangés. Les 57 assertions de l’éditeur utilisent le vrai
Worker/WASM. Clippy Windows/WASM, format, licences et builds Windows/WASM/musl
passent ; les tests musl sont compilés localement et exécutés en CI Linux.
La [copie portable](../_bmad-output/implementation-artifacts/styles-formats-raster-portable.json)
fonctionne avec le seul atelier.exe et ses 18 ressources embarquées.
Les limites des US parentes et l’échec strict de handles US-111 restent ouverts.
