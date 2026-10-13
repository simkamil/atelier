# Contrat binaire du remplacement TXT

Le remplacement est limité à l’éditeur texte. Il utilise l’enveloppe ATED v1 et les mêmes options, frontières Unicode et règles de normalisation que la recherche Rust.

## Commandes ATED

Les opcodes sont `21` (remplacer la sélection de recherche) et `22` (remplacer tout). Tous les entiers sont little-endian. Les deux corps commencent par `revision:u64`, `nowMs:u64`, `flags:u8`, `reserved:u8=0`, `reserved:u16=0`, `queryLen:u32`, `replacementLen:u32`. `flags` reprend les bits de recherche : casse `1`, mot entier `2`, ignorer les accents `4`, faire correspondre les espaces insécables `8`. Les bits inconnus sont refusés.

L’opcode 21 ajoute `start:u32,end:u32`. Les octets UTF-8 de la requête et du remplacement suivent immédiatement, dans cet ordre. L’opcode 22 ne contient pas de plage. Le corps doit finir exactement après les deux chaînes. La requête est non vide et limitée à 1 024 octets ; le remplacement est limité à 128 Kio. UTF-8 invalide, longueur incohérente, octets réservés non nuls et options inconnues sont rejetés.

Une révision périmée renvoie `EDITOR_STALE_QUERY`. Pour l’opcode 21, la plage doit correspondre à la sélection actuelle et à une occurrence exacte de la requête selon les options ; sinon la réponse est `EDITOR_STALE_MATCH`. Une requête vide produit `EDITOR_EMPTY_QUERY`.

ReplaceAll parcourt uniquement les occurrences de la source immuable, y compris au-delà des 1 000 résultats exposés par l’interface de recherche. Il calcule la taille résultante avant d’allouer le texte candidat. Le candidat complet doit rester inférieur ou égal à 128 Kio. Si le contenu change, le moteur remplace l’enveloppe des plages trouvées par un seul appel à `replace_range_isolated`, puis calcule un seul layout final. Une erreur de budget, d’historique, de révision ou de layout ne publie pas la session candidate. Aucun match et résultat identique n’ajoutent pas d’historique et ne changent pas la révision ; le compte retourne néanmoins les occurrences correspondantes.

## Réponse ATQR

La réponse `EditorResult` contient `ATQR`, `version:u16=1`, `reserved:u16=0`, `count:u32`, `stateLen:u32`, puis exactement `stateLen` octets d’une réponse ATES complète. Le compte ne dépasse pas 131 072. La réponse totale respecte la limite de 32 Mio. Le décodeur vérifie aussi strictement l’état ATES imbriqué et refuse les octets surnuméraires.

En JavaScript, l’encodage renvoie les opcodes 21/22 avec `{revision, nowMs, query, replacement, options, start, end}` pour Replace et sans `start/end` pour ReplaceAll. Le décodage renvoie `{kind: "replacement", count, state}` ; l’intégration publie `state` et transmet `count` à l’interface.
