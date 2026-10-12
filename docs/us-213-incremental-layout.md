# US-213 — Cache de paragraphes et pagination réutilisable

`atelier_text_layout::incremental::IncrementalSession` conserve les résultats
de mise en page par identité stable de paragraphe. Chaque appel à `update`
reçoit la séquence complète ordonnée (`ParagraphInput`), ses propriétés
résolues, les règles de pagination, les options de layout et de pagination, les
tabulations et le `FontManager`.

```rust
let result = session.update(&paragraphs, &fonts, options)?;
let pages = &result.pagination.pages;
let work = &result.stats;
```

Le cache compare directement, sans hash, l’ID, le paragraphe complet (texte,
runs et propriétés), les propriétés résolues, les options de layout, les
tabulations et la révision globale du gestionnaire de polices. La révision
`FontManager::cache_revision()` est distincte par gestionnaire et change après
une mutation réussie des polices. Les règles de pagination sont comparées
séparément puisqu’elles ne changent pas la géométrie des lignes.

Un appel identique renvoie le résultat publié, sans reshaping ni exécution du
flux. Une édition locale reshapes uniquement les paragraphes dont la clé
sémantique diffère. La pagination repart d’un checkpoint avant la première page
susceptible de changer. Le recul inclut un paragraphe précédent pour les
ajustements veuve/orpheline et remonte les dépendances `keep_with_next`. Le
paginateur partagé enregistre l’état réel avant chaque paragraphe. Après la
dernière entrée modifiée, il s’arrête dès que la position source, la page
ouverte, son contenu, l’espacement précédent, le saut en attente et les valeurs
de chaîne de conservation concordent avec le calcul précédent. Il copie alors
la suite des pages et diagnostics sans exécuter le flux pour cette suite.

La prévalidation des hauteurs, des sauts et des chaînes de conservation reste
linéaire sur le document entier; elle est exposée séparément dans
`pagination_preflight_work`. `pagination_stream_work` compte le flux réellement
exécuté. `pages_recalculated` compte les pages touchées par ce flux, y compris
la page ouverte au point de couture; `pages_reused` est le reste. Les compteurs
sont issus des frontières effectivement traversées et des pages effectivement
copiées.

Une insertion, suppression ou réorganisation des IDs impose une pagination
complète, mais les paragraphes de géométrie identique restent réutilisables.
Les changements de largeur, tabulations ou révision de police invalident le
shaping; un changement des options globales de pagination conserve les layouts
de paragraphes mais repagine le document. Si aucun checkpoint stable n’est
représentable, le flux reprend au checkpoint sûr et s’exécute jusqu’à la fin;
`fallback_reason` le signale. Les IDs en double sont rejetés.

Une erreur de layout ou de pagination ne publie rien : le résultat et le cache
antérieurs restent consultables par `session.result()`. Les limites et
l’arithmétique du paginateur complet s’appliquent au préflight et au flux
effectivement exécuté.

## Mesure

L’exemple `cargo run -p atelier-text-layout --example incremental_benchmark
--release --locked --offline` construit 300 pages de hauteur utile A4
(44 800 unités de 1/64 pt, arrondies vers le bas à une ligne entière), largeur
30 000 unités et 300 paragraphes, chacun contenant autant de lignes que la
hauteur utile permet. Il mesure l’initialisation complète puis une édition au
milieu du document, publie les deux temps et tous
les compteurs, sans assertion temporelle fragile.

Ces temps décrivent ce workload sur la machine où l’exemple s’exécute. Ils ne
mesurent pas une frappe utilisateur complète et ne démontrent aucune cible
d’interface inférieure à 16 ms. La validation de cette cible nécessite une
mesure de frappe réelle sur la machine de référence §6.16.

Mesure de ce lot : 1912.351 ms pour le premier calcul, 268.214 ms pour l’édition locale, un paragraphe reshaped, 299 réutilisés, 3 pages recalculées et 297 réutilisées. Le chronométrage porte sur `update` sans clone externe du résultat. Prévalidation : 16 500 unités; flux exécuté : 217. Voir les [valeurs et limites](../_bmad-output/implementation-artifacts/rich-incremental-borders-benchmark.json) et la [machine](../_bmad-output/implementation-artifacts/rich-incremental-borders-host.json). La cible de 16 ms reste non atteinte sur cet hôte. US-213 reste partielle pour la performance et le raccordement UI riche.
