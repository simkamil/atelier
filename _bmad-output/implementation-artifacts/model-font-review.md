<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Revue US-050, US-073 et US-074 — 2026-10-10

Trois agents neufs GPT-6 Luna moyen ont relu en parallèle le diff initial
`1d3e05e59a5568d7a02709622bc9168fd411f1e7c6777ead2be60cc28b5ba696`
(3 916 512 octets, graphe généré inclus), depuis le commit `77622b0`.
Les trois retours ont été collectés avant ce tri. Aucun changement produit
n'a été fait pendant leurs relectures. Les contrôles indépendants Python et
FontTools sont distincts des tests Rust exécutés par la CI.

## Review Triage Log

Chaque ligne constitue un verdict individuel avant regroupement.

| ID | Constat | Verdict | Preuve et route |
|---|---|---|---|
| B1 | Nom UTF-16 de longueur impaire tronqué | medium | `chunks_exact(2)` perd le dernier octet. Correction locale : refuser la longueur impaire, conserver U+FFFD pour les unités invalides. patch |
| B2 | Records de langues `name` version 1 non validés | medium | `parse_names` ne lit que les records ordinaires ; une version 1 sans langTagCount passe. Valider les plages et le stockage des tags. patch |
| B3 | Sélecteurs format 14 non triés, dupliqués ou invalides | medium | `validate14` ne contrôle que le plafond Unicode. Les sélecteurs ordinaires ou doublons rendent la résolution ambiguë. Gardes locales. patch |
| B4 | Fin de plage UVS hors Unicode | medium | Seul le début est contrôlé ; `additionalCount` peut prolonger U+10FFFF. Contrôler fin et ordre des plages. patch |
| B5 | Offset UVS pointant dans les records | medium | Une valeur non nulle peut référencer l'en-tête tant que la lecture du compte réussit. Contrôler l'origine des données et leurs bornes. patch |
| B6 | Mapping non-default sans cmap de base | false | La spécification OpenType autorise explicitement l'absence du caractère de base dans cmap pour un non-default UVS. Rejeter ce cas casserait des polices conformes. Aucune correction. |
| B7 | Formats cmap non implémentés ignorés | low | L'API n'expose que 4/6/12/14 et ne lit pas les autres formats. Aucun accès non borné ; préciser la documentation trop générale plutôt que décoder des formats supplémentaires. patch documentaire |
| B8 | Format 14 accepté sous une plateforme incorrecte | medium | `parse_cmap` affecte variation avant le classement plateforme. Exiger Unicode plateforme 0, encodage 5 pour le format 14. patch |
| B9 | Tableau d'offsets TTC incomplet accepté | medium | Seul l'offset de la face demandée est lu ; le compte complet peut déborder du buffer. Valider le tableau complet avant sélection. patch |
| B10 | Archive CI datée prétendument périmée | false | `github-ci-editing.json` conserve le SHA exact 77622b0 et les conclusions de son exécution. C'est une preuve historique immuable du lot précédent, pas le statut courant. La documentation identifie les commits. |
| E1 | Caractère ordinaire accepté comme sélecteur | medium | Même défaut que B3, vérifié à `validate14` : comparaison au seul plafond Unicode. patch |
| E2 | Octet UTF-16 final perdu | medium | Même défaut que B1, vérifié à `parse_names` : `chunks_exact(2)`. patch |
| G1 | Absence de test public de format 14 en CI | medium | Aucun des cinq tests initiaux n'appelle `variation_glyph`. Le comparateur externe confirme huit rejets de polices conformes. Ajouter des fixtures synthétiques et un test public exécuté par Cargo. patch |
| R1 | Longueur format 14 lue au mauvais offset | high | Le champ uint32 commence à +2, le code le lit à +4 ; les huit faces produites par FontTools sont rejetées. Correction locale du champ, test de non-régression. patch |
| R2 | Temps de validation UVS amplifiable par réutilisation | high | Un grand nombre de sélecteurs peut référencer le même grand tableau ; la limite de chaque compte ne borne pas la somme des itérations. Décompter un budget global et éviter de revalider un même sous-tableau cmap. patch |
| R3 | Tables maxp/post tronquées acceptées | medium | maxp 1.0 à six octets et post à quatre octets passent. Vérifier minima par version et les longueurs des variantes post exposées comme tables valides. patch |
| R4 | Cmap secondaires utilisés après absence dans le préféré | medium | `glyph_index` parcourt les autres formats si le préféré ne contient pas le caractère. OpenType demande de sélectionner un seul cmap de base. Garder le choix 12 puis 4 puis 6 et retourner zéro sans mélange. patch |
| R5 | Noms Windows legacy 3/4/5 traités comme UTF-16 | medium | La spécification name impose les pages 936/950/949 pour ces trois encodages ; le décodeur UTF-16 peut perdre ou refuser leurs octets. Utiliser la représentation octet-vers-caractère déjà documentée pour ces encodages non interprétés. patch |
| R6 | Autonomie des ressources vérifiée seulement hors CI | medium | Les sept tests initiaux du journal utilisent un codec d’arbre sans ressource ; l’oracle externe vérifie un payload avec ressource. Ajouter un test Rust qui conserve ressources et état de base malgré remplacement de l’état source. patch |
| G2 | Mutations externes limitées aux sfnt | low | La revue complémentaire des outils confirme que le glob excluait TTC et que le pilote appelait seulement parse. Correction directe : inclure two-faces.ttc et exercer parse_collection aux indices 0 et 1 pour chaque entrée. patch appliqué |

## Regroupement et traitement

B1/E2 partagent la troncature UTF-16 ; B3/E1 partagent les sélecteurs invalides.
Les autres constats conservent leurs causes distinctes. B6 et B10 sont réfutés.
La revue complémentaire des trois outils d’oracle a signalé G2 ; aucun autre défaut vérifié de leurs références ou comparaisons JSON. Les corrections sont confiées à l'agent d'implémentation original, sans nouvelle
surface publique. Les modifications documentaires appartiennent au coordinateur.
Toutes les corrections ci-dessus sont appliquées et validées ; B6/B10 sont réfutés. Aucun constat différé. Le test de langue de nom utilise des octets UTF-16BE conformes. Les 14 contrôles locaux passent (187 tests workspace, 51 release) et les comparaisons indépendantes passent : 8 faces, 25 272 mutations/troncatures, 12 001 sélections, 267 cas de journal. Les sources et le lock ne changent pas pendant les contrôles. Les fixtures versionnées sont régénérées à l’identique. Voir model-font-validation.json et model-font-oracle.json.

Références normatives : [cmap et UVS](https://learn.microsoft.com/en-us/typography/opentype/spec/cmap),
[name](https://learn.microsoft.com/en-us/typography/opentype/spec/name),
[post](https://learn.microsoft.com/en-us/typography/opentype/spec/post).
