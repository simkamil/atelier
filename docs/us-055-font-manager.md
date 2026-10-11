# US-055 — Gestionnaire de polices

`atelier_font::manager` fournit un registre déterministe de faces OpenType empruntées. Il ne lit pas le disque et ne découvre pas les polices du système. L’appelant transmet les octets et la provenance (`FontSource::System` pour les octets fournis par le Host, `Document` pour l’importeur ou `Embedded` pour un catalogue). Le module ne dépend pas du système d’exploitation et ne modifie jamais les données source.

`FontManager::new()` parse les vingt faces de `EMBEDDED_FONTS`. `FontManager::empty()` démarre sans face; `FontManager::with_limits(FontManagerLimits { max_faces, max_total_bytes })` permet aux tests et intégrateurs de définir des plafonds. Les valeurs par défaut sont 1024 inscriptions et 256 MiB cumulés. Chaque inscription TTC facture la taille complète du conteneur. `register(bytes, face_index, source)` valide les limites avant le parseur, conserve les octets empruntés et renvoie un `FontId` monotone. Une erreur ne modifie ni le registre ni les budgets. `remove(id)` libère sa place et son budget; les autres identifiants restent valides et l’identifiant supprimé n’est jamais réutilisé.

Une `FontRequest` contient une liste ordonnée de familles, un poids entre 1 et 1000, un style (`Normal`, `Italic`, `Oblique`) et éventuellement un générique (`SansSerif`, `Serif`, `Monospace`). `resolve` renvoie `ResolvedFont`, qui expose l’identifiant, la `FontFace`, la famille effective, le poids, le style, la source et l’index de face TTC. `resolve_for_char` applique le même ordre après avoir écarté les faces dont le cmap renvoie `.notdef`; si aucune face ne couvre le caractère, il renvoie `ManagerError::MissingGlyph`.

Les clés famille viennent des noms Unicode ID 16 (typographiques) s’ils existent, sinon ID 1. Le nom affiché préfère l’anglais Unicode Windows (`en-US`), puis choisit une autre variante Unicode selon plateforme, langue et valeur. Les clés normalisent les espaces et passent en minuscules Unicode. La correspondance essaie les familles demandées dans l’ordre, chacune avec son alias métrique éventuel, puis le générique. Les alias sont Calibri→Carlito, Cambria→Caladea, Arial→Liberation Sans, Times New Roman→Liberation Serif et Courier New→Liberation Mono; les génériques pointent respectivement sur Carlito, Caladea et Liberation Mono. Si un générique n’est pas installé, la première autre famille enregistrée sert de repli.

Pour une famille, la source suit l’ordre Document, System, Embedded. À source égale, le style exact précède la proximité Normal→Oblique→Italic, Italic→Oblique→Normal ou Oblique→Italic→Normal. Le poids suit la recherche CSS Fonts : sous 400, poids inférieurs décroissants puis supérieurs croissants; au-dessus de 500, supérieurs croissants puis inférieurs décroissants; de 400 à 500, vers 500 puis vers le bas, puis au-dessus de 500. Les égalités conservent l’ordre d’inscription. Le poids vient de `OS/2.usWeightClass` (400 si absent) et le style de `OS/2.fsSelection`, avec repli sur `head.macStyle` pour l’italique.

Le gestionnaire ne synthétise pas le gras ou l’italique, ne fait pas de shaping et ne met pas en cache les glyphes. Le pont Host/natif prévu par US-113 reste hors de cette API.

Références primaires : [OpenType name](https://learn.microsoft.com/en-us/typography/opentype/spec/name), [OS/2](https://learn.microsoft.com/en-us/typography/opentype/spec/os2) et [CSS Fonts4, recherche de poids](https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm). Les tests couvrent les cinq alias dans quatre variantes, les intervalles CSS, les sources/TTC, la couverture et les inscriptions atomiques. Le contrôle Windows lit réellement les quatre variantes Arial installées et vérifie leur sélection System ; il ne distribue aucun fichier Microsoft. Sa [preuve](../_bmad-output/implementation-artifacts/text-layout-system-fonts.json) distingue cette API du pont natif US-113 encore ouvert.

`ManagerError::EmptyRegistry` signifie qu'aucune face ne satisfait la requête de familles, y compris si des faces d'autres familles sont enregistrées. `is_empty()` distingue ce cas du registre réellement vide. Les quatre résultats du contrôle système sont :

| Famille effective | Poids | Style | Source |
|---|---:|---|---|
| Arial | 400 | Normal | System |
| Arial | 700 | Normal | System |
| Arial | 400 | Italic | System |
| Arial | 700 | Italic | System |

Validation du lot : [26 contrôles](../_bmad-output/implementation-artifacts/text-layout-validation.json), 291 tests workspace et 101 tests release sur sources inchangées ; [revue indépendante triée](../_bmad-output/implementation-artifacts/text-layout-review.md), aucun constat différé.
