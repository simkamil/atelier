<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-074 — Journal de récupération

`atelier_model::journal` fournit une enveloppe de snapshot et un journal binaire
de transactions `ATOP` pour une seule génération de récupération. Il ne dépend
pas d'un format de document : le `SnapshotCodec<S>` appelant encode toutes les
ressources nécessaires dans un payload autonome. Un document sans fichier
source se représente par un payload vide ou par le format choisi par ce codec.

## Identités et enveloppes

Les identifiants document et génération sont deux tableaux opaques de 16 octets.
Les deux enveloppes utilisent la version little-endian `u16` égale à 1.

Le snapshot est : `ATJS`, version, ID document (16), ID génération (16), séquence
de base `u64` little-endian, taille payload `u64` little-endian, payload exact,
puis CRC32 `u32` little-endian. Le CRC couvre tous les octets précédents de
l'enveloppe. La séquence de base doit égaler celle du journal. La taille totale
par défaut est plafonnée à 64 Mio.

Le journal est : `ATJR`, version, les deux IDs, puis séquence de base `u64`
little-endian. Chaque entrée suit sous la forme taille de frame `u32`, séquence
`u64`, taille ATOP `u32`, octets ATOP, CRC32 `u32`. La taille de frame inclut la
séquence, la taille ATOP, ses octets et le CRC. CRC32/ISO-HDLC (polynôme réfléchi
`0xEDB88320`, initial et final XOR `0xFFFFFFFF`) couvre la séquence, la taille
ATOP et les octets ATOP, sans le préfixe de frame ni le CRC lui-même.

Les séquences commencent à `base + 1` et sont contiguës. Un dépassement de
séquence `u64` refuse l'ajout. Le journal complet est plafonné par défaut à
64 Mio et 100 000 entrées. `JournalLimits` permet de réduire ou augmenter ces
plafonds côté appelant; aucune valeur lue sur disque ne peut les modifier.
Chaque ATOP conserve ses limites propres (1 Mio, 4 096 opérations et 64 Kio par
opération). L'append encode et calcule toutes les longueurs avant de modifier
les octets du journal. Il ne compresse pas les transactions.

## Rejeu

`SnapshotCodec::restore` retourne un nouvel état, ce qui garde une restauration
échouée hors de l'état exposé. Le moteur vérifie d'abord les deux en-têtes et
leurs IDs, le CRC de l’instantané et la même séquence de base. Une différence d'identifiant,
de séquence ou une version inconnue est une erreur globale. Le codec doit inclure
dans son payload les ressources requises et valider celles-ci; le moteur ne peut
pas vérifier leur autonomie sémantique.

Le rejeu part exclusivement de l'état restauré du snapshot fourni. Pour chaque
entrée, il valide le frame complet, les bornes, la séquence et le CRC, décode
entièrement `Transaction::decode`, puis appelle `Transaction::apply`. Ainsi une
transaction refusée conserve le checkpoint exact précédent. Au premier défaut,
le rejeu s'arrête et retourne cet état, la dernière séquence appliquée, le nombre
d'entrées appliquées et `StopReason`; aucune entrée ultérieure n'est consultée.
Une queue tronquée est donc récupérée au dernier enregistrement entier et valide.

Exemple simplifié :

```rust,ignore
let snapshot = snapshot_state(&codec, &state, document_id, generation_id, base_sequence, limits)?;
let mut journal = Journal::new(document_id, generation_id, base_sequence, limits)?;
journal.append::<DocumentState, DocumentOperation>(&transaction)?;
let result = replay::<DocumentState, DocumentOperation, _>(
    &codec, &snapshot, journal.as_bytes(), limits,
)?;
```

Le CRC détecte les corruptions accidentelles, sans authentifier le contenu.
Cette API manipule des octets en mémoire et ne garantit ni flush/fsync, ni
publication atomique, ni planification, ni suppression de génération. Ces
responsabilités et la détection/proposition de récupération appartiennent à
US-325 et au stockage appelant. Le journal de récupération est distinct de
l'historique interactif undo/redo.

Les preuves de validation et les limites d’intégration sont rassemblées dans le [suivi du lot](MODEL-FONT-PROGRESS.md).
