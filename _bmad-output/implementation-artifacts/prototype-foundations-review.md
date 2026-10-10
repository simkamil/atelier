# Revue US-010/051/090 — 2026-10-10

Trois agents indépendants GPT-6 Luna, raisonnement moyen, ont examiné le diff
complet depuis `20a6a05bd69d22f4f004b7a907d2c9696b24c4ad`, sources nouvelles
comprises. Chaque constat a été vérifié et trié individuellement avant regroupement.
Les corrections ont été confiées aux agents d’implémentation d’origine.

| ID | Constat | Verdict et preuve | Disposition |
|---|---|---|---|
| B1 | rename ne remplace pas un fichier sur Windows | false : deux générations CLI successives vers le même fichier existant ont réussi sur Windows ; Rust std prend en charge son remplacement. | Rejeté, aucun contournement destructif. |
| B2 | Commentaire is_emoji confondu avec Extended_Pictographic | low : la fonction lit bien Emoji, mais son commentaire nomme une autre propriété. | Correction directe du commentaire. |
| B3 | Valeurs de propriétés invalides acceptées | medium : push_property accepte une chaîne arbitraire dans LB/GCB/WB/InCB ; une édition invalide peut être publiée. | Validation contre PropertyValueAliases figé et tests négatifs. |
| B4 | 256 valeurs refusées par un index u8 | low : >255 exclut la frontière256 représentable, bien que les propriétés17 réelles soient plus petites. | Correction directe de la borne. |
| B5 | Comparaison Unicode exhaustive hors CI | medium : tests natifs représentatifs et reproductibilité peuvent accepter une erreur cohérente générateur/sortie. | Empreintes indépendantes de tous les scalaires vérifiées en test natif. |
| B6 | Mappings de casse partiellement couverts en test natif | medium : les dix consultations ne sont pas toutes comparées aux références dans les tests existants. | Même test exhaustif, empreinte séparée par consultation. |
| B7 | Séquences de casse conditionnelle peu vérifiées en CI | medium : seule la présence d’une condition ne détecte pas une permutation des séquences. | Empreinte de toutes les colonnes des seize règles officielles. |
| B8 | Erreurs de flags/coordonnées/transforms non ciblées | medium : corpus valide et oracle ne prouvent pas ces branches de rejet. | Mutations ciblées avec attentes d’erreurs typées dans outlines.rs. |
| B9 | Budgets imbriqués non testés aux frontières | medium : le test composant nul ne vérifie pas le cumul de points/contours/profondeur. | Composite imbriqué, totaux exacts acceptés et un de moins refusé par budget. |
| B10 | Gardes du chargeur sur descripteur/payload malformé non testées | medium : les exports normaux ne produisent pas ces cas, mais les branches protègent une frontière binaire. | Doubles contrôlés des exports pour erreurs/cleanup, croissance, reset raté ; tests réels WASM conservés. |
| E1 | Ligne de propriété malformée ignorée | medium : le continue de property_file ignore aussi des lignes non vides à un champ. | Rejet explicite des lignes noncommentaires invalides et test. |
| V1 | Oracle exhaustif absent de la CI | medium : constat de vérification pré-vérifié par les recherches du reviewer. | Couvert par le test natif d’empreintes indépendantes, sans Python en CI. |
| R1 | Réinitialisation fatale conserve l’ancienne instance si instantiate échoue | medium : la réaffectation de state attendait le retour de instantiate ; une validation d’exports ratée laissait donc l’ancienne instance accessible. | Instance abandonnée avant la tentative, EngineResetError avec cause, tests des deux issues et absence de rejeu. |
| R2 | Seuil affiché arrondi au-dessus du seuil entier accepté | low : l’affichage ceil annonçait57556, mais le test exact +10% accepte jusqu’à57555. | Affichage floor cohérent avec le contrôle et documentation corrigée. |

Les corrections passent les 21 contrôles finaux, dont 233 tests workspace
et 43 tests release, dans `prototype-foundations-validation.json`. Aucun constat
n’est différé. Les tests musl sont compilés localement ; leur exécution est assurée
par le job Linux de la CI du commit publié.
