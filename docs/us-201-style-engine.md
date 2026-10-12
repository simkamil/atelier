<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-201 — Moteur de styles

`atelier_text_model::rich::StyleEngine` calcule les propriétés d’un paragraphe
et d’un run sans toucher à `PlainTextDocument`. Le modèle reste indépendant de
l’éditeur et du WASM.

## Contrat d’appel

Construire un `StyleCatalog` avec les valeurs de document, puis ajouter les
styles par identifiant. Les références `based_on`, les styles de table et les
références du thème sont vérifiés au moment de la résolution ; les styles peuvent
donc être chargés dans n’importe quel ordre. Le catalogue expose
des styles de paragraphe, caractère ou tableau, chacun avec des propriétés
`CharAttrs` et `ParaProps`. `StyleCharAttrs` accepte en plus les emplacements de
police et de couleur du thème.

```rust,ignore
let mut engine = StyleEngine::new(catalog, theme, 512);
let run_direct = paragraph.attrs_at(offset);
let calculated = engine.resolve(&document, paragraph_id, Some("Accentuation"), &run_direct)?;
```

La résolution applique les valeurs de document, le style de tableau qui
contient le paragraphe, la chaîne de styles de paragraphe `based_on`, le style
de caractère transmis par l’appelant pour le run, puis les attributs directs.
Un style lié est une paire indépendante : le consommateur choisit le style de
paragraphe pour le paragraphe ou son partenaire caractère pour les runs
sélectionnés, au moyen de `linked_style_id`, puis passe le choix caractère à
`resolve`. Le lien n’est pas appliqué automatiquement. Un partenaire absent ou
de mauvais type est ignoré. Les valeurs concrètes d’un style remplacent ses références
thème équivalentes. Chaque propriété est fusionnée indépendamment : `Some(false)`
reste une valeur définie, `Underline::None` supprime explicitement le souligné,
et une propriété absente hérite du niveau précédent. Les propriétés directes
sont passées séparément à `resolve` afin que les runs du même paragraphe ne
partagent pas leur mise en forme.

Dans une chaîne `based_on`, la valeur explicite la plus proche est celle du
style enfant. Pour `bold`, `italic` et `strike`, Word réinitialise l’état à la
valeur effective d’un style de tableau ou de paragraphe quand elle est présente.
Pour le niveau caractère, le moteur applique la bascule OOXML : quand le défaut
du document est faux, une valeur effective vraie bascule l’état et faux le
laisse inchangé ; quand le défaut est vrai, une valeur effective fausse bascule
l’état et vrai le laisse inchangé. Ainsi un défaut vrai n’est pas garanti de
rester vrai après les niveaux de style. La mise en forme directe reste absolue :
`Some(false)` éteint explicitement la propriété, et `Some(true)` l’allume.
Cette normalisation suit les notes Word sur les défauts vrais et les styles de
tableau [MS-OI29500 §2.1.230a](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/f7130225-2368-48f3-acae-a9d278d0fb25)
et [MS-OI29500 §2.1.246b](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/14452bbe-be4d-4dbb-90e6-3d23ae9361bc) ;
Word réinitialise également une propriété explicitée par un style de paragraphe
[MS-OE376 §2.1.260](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376/f936abaf-a9cb-439b-923d-7f688d9202e9).

Un tableau conserve son identifiant de style dans `Table::style`.
Une table imbriquée sans style propre reprend le style explicite le plus proche
dans ses tables ancêtres, puis le style de tableau par défaut du catalogue.
La comparaison de ce cas précis avec un rendu Word de référence reste à faire
(constat R6 de la revue, divergence non établie).
Utiliser
`RichOp::SetTableStyle` pour le modifier : l’opération est inversible et le
moteur invalide les entrées de cache des paragraphes descendants après son
application réussie. Utiliser `StyleEngine::apply` pour les opérations locales
qui changent des propriétés. En cas d’échec, la transaction restaure le document
et le cache est conservé. `set_style` invalide uniquement les résultats qui
dépendent du style modifié ; un changement de thème ou de valeurs par défaut vide
le cache. Sa taille maximale est fournie au constructeur (`0` désactive le
cache).

Les cycles `based_on`, les styles absents ou de mauvais type et les emplacements
de thème absents donnent une `StyleError` sans boucle. La résolution n’installe
pas de résultat partiel dans le cache en cas d’erreur. Les mutations faites en
dehors de `StyleEngine::apply` doivent appeler `clear_cache` si elles changent
un contexte de style déjà résolu.

## Preuves ciblées

`cargo test -p atelier-text-model --locked --offline` couvre la cascade complète,
les styles liés, les bascules `bold`, `italic` et `strike` dans `based_on`, les
valeurs indépendantes, les valeurs `false` et underline explicite, les références
de thème, les tables, les styles absents, les cycles,
le cache borné, les changements locaux et partagés, l’annulation par opération
et la conservation du cache après une opération refusée.

Références du contrat Word : [style lié](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.linkedstyle?view=openxml-3.0.1), [propriétés à bascule](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376/f936abaf-a9cb-439b-923d-7f688d9202e9).

## Validation d’intégration — 2026-10-12

La [passe finale](../_bmad-output/implementation-artifacts/styles-formats-raster-validation.json)
réussit 41 contrôles, 507 tests workspace et 55 tests natifs release,
avec sources et lock inchangés. Les 57 assertions de l’éditeur utilisent le vrai
Worker/WASM. Clippy Windows/WASM, format, licences et builds Windows/WASM/musl
passent ; les tests musl sont compilés localement et exécutés en CI Linux.
La [copie portable](../_bmad-output/implementation-artifacts/styles-formats-raster-portable.json)
fonctionne avec le seul atelier.exe et ses 18 ressources embarquées.
Les limites des US parentes et l’échec strict de handles US-111 restent ouverts.
