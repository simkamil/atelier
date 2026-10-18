# ZIP/XML validés — 2026-10-10

**US-022, US-023, US-030 et US-031 sont terminées : 14 US sur 158.** Les implémentations ont été développées en parallèle avec GPT-6 Luna, raisonnement moyen, puis examinées par trois agents neufs du même modèle.

- [ZIP](us-022-023-zip.md) : central classique/Zip64, Store/Deflate, UTF-8/CP437, limites, CRC, descripteurs 32/64 bits signés/non signés et erreurs par entrée. Écriture déterministe, ordre contrôlé et date fixe. Les entrées rejetées ne masquent pas les homonymes saines ni leur budget.
- [XML](us-030-031-xml.md) : lecture incrémentale UTF-8/UTF-16, namespaces résolus, références, commentaires/PI/CDATA, erreurs positionnées et tolérance EOF explicite. Budgets avant allocation, DTD sans expansion, sortie déterministe et writer équilibré.

## Preuves

140 tests du workspace passent; les 21 tests ZIP et 28 tests XML passent aussi en release. Les [treize contrôles globaux](../_bmad-output/implementation-artifacts/formats-validation.json) réussissent : Windows/WASM/musl, tests natifs, Clippy, rustfmt, SPDX, garde et métadonnées Cargo. Les tests Linux sont compilés sur Windows; le [workflow distant](https://github.com/simkamil/atelier/actions/workflows/ci.yml) les exécute sur Linux musl.

L'[oracle indépendant](../_bmad-output/implementation-artifacts/format-oracle.json) valide 105 lectures et 105 réécritures XML, 20 rejets, 52 lectures ZIP et 48 écritures ZIP. Le [gestionnaire ZIP Windows](../_bmad-output/implementation-artifacts/explorer-zip.json) extrait correctement les variantes classique et Zip64. Les empreintes identifient les sources exactes testées.

Les [trois revues](../_bmad-output/implementation-artifacts/formats-review.md) ont été triées constat par constat. Les correctifs portent sur le refus préalable du prochain événement XML, les diagnostics de doublons ZIP, la copie mémoire inutile du writer et la protection CI de son format binaire. Aucun constat différé. La récupération d'archives sans central reste volontairement limitée aux frontières locales fiables; elle ne cherche jamais une signature dans les données.

Le corpus contient 25 fixtures avec manifest SHA-256, dont deux sorties ZIP Writer validées indépendamment par Python et comparées octet par octet dans les tests Rust CI. Python et PowerShell restent des outils d'agent. Les autres 23 fixtures sont inchangées; le générateur reproduit le corpus sans dépendance externe.
