# US-092 — Moteur WASM dans un Web Worker

Le module [engine-client.mjs](../web/engine-client.mjs) expose `createEngineClient({ module })` ou `createEngineClient({ bytes })`. Il crée un Worker ES de production si aucun `worker` de test n’est fourni, initialise une instance WASM privée, puis attend son message `ready` avant de retourner le client. Une initialisation silencieuse dépasse au plus 30 secondes avant rejet et arrêt du Worker.

`client.request(commandFrame, { signal, onProgress, chunkBytes })` accepte une trame de commande v1 en `Uint8Array` et retourne une promesse de trame écho exacte. L’ID BigInt est lu avec le codec v1 et reste actif jusqu’à la réponse. Un ID déjà actif, un AbortSignal déjà annulé ou une trame invalide est refusé. Pour annuler, appelez `AbortController.abort()` : le client envoie une enveloppe de contrôle et une trame v1 `Cancel` dont l’ID est celui de la commande ciblée. La promesse rejette avec `AbortError` et aucun résultat tardif n’est remis à l’appelant.

`onProgress` reçoit `{ phase, copiedBytes, totalBytes }`. `phase` vaut `input`, `output` ou `complete`; les compteurs décrivent les octets réellement copiés. Les exceptions du callback sont isolées. `chunkBytes` accepte un entier de 16 KiB à 16 MiB et vaut 256 KiB par défaut, ce qui borne le nombre de notifications de progression. `client.close()` rejette les appels en attente avec `WorkerClosedError`, retire les listeners et arrête le Worker. Une erreur fatale du Worker règle de la même façon toutes les promesses en attente.

Le Worker ne transfère que le tampon JavaScript de résultat, alloué une fois puis rempli directement depuis la mémoire WASM par blocs. Le transfert détache ce tampon dans le Worker. La mémoire WASM reste privée. `dispatchAsync` cède à la file des tâches entre les blocs d’entrée et de sortie; l’appel WASM synchrone lui-même n’est pas préemptible. Une seule opération utilise une instance à la fois. Pour les appels directs à `dispatchAsync`, le buffer d’entrée doit rester attaché et à la même taille jusqu’à la résolution; un détachement ou redimensionnement pendant la copie est signalé et l’allocation WASM est libérée. L’API synchrone `dispatch` reste disponible.

Les admissions du client sont limitées à 32 requêtes simultanées et 128 MiB de trames retenues. Le runtime borne aussi sa file en attente à 32 requêtes et 128 MiB; la requête active est comptée séparément par le client mais retirée de la file runtime. Chaque trame v1 reste soumise à la limite du codec, soit 64 MiB moins quatre octets. `chunkBytes` est plafonné à 16 MiB. Ces limites couvrent les trames en file, pas la mémoire nécessaire à l’entrée et à la sortie d’une requête active.

## Intégration

```js
import { createEngineClient } from "./engine-client.mjs";

const response = await fetch("./atelier_core_wasm.wasm");
const client = await createEngineClient({ bytes: await response.arrayBuffer() });
const outputFrame = await client.request(commandFrame, {
  signal: abortController.signal,
  onProgress: ({ phase, copiedBytes, totalBytes }) => updateProgress(phase, copiedBytes, totalBytes),
});
client.close();
```

Le client transporte les anciens types par écho ABI de US-090 et les commandes métier `Editor`/`EditorResult` de l’[éditeur intégré](us-120-editor-ui.md). Les sondes historiques ci-dessous vérifient l’écho ; les tests `editor-abi.mjs` et `run-editor-browser.mjs` exercent la session réelle. La validation Node ne valide ni clavier physique/IME ni WebView2. L’annulation des copies ne préempte pas le dispatch WASM synchrone.

## Vérification

```sh
node tests/wasm-worker.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm
node tests/wasm-abi.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm target/wasm-abi-hooks/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm
```

Le test Worker vérifie les IDs u64 au-delà de `2^53`, l’ordre FIFO, les sous-vues, la copie progressive, l’annulation après une progression partielle, la reprise, la fermeture et le détachement observé après le vrai `postMessage` avec transfert.

## Opérations longues : ouverture et export annulables

Un appel WASM n'est pas préemptible. Les opérations longues sont donc découpées côté moteur en étapes bornées ([staged.rs](../crates/core-wasm/src/staged.rs), [rich_preview.rs](../crates/core-wasm/src/rich_preview.rs) pour le rendu) et pilotées par le Worker, qui cède à la file des tâches entre deux étapes. Le message `cancel` (donc `AbortSignal`) est traité à ce point de coopération.

Opcodes `Editor` ajoutés (ABI inchangée : même point d'entrée `dispatch`, ajout rétrocompatible d'opcodes 23 à 25 ; les opcodes existants et leurs réponses ne changent pas) :

- 23 `Begin` : `u16 kind` (1 ouverture DOCX avec aperçu paginé, 2 export DOCX), `u16 0`, octets DOCX (max 8 MiB). Remplace et libère tout job abandonné. Réponse `ATJB`.
- 24 `Step` : `u64` id du job. Réponse `ATJB` (en cours) ou le résultat final.
- 25 `Cancel` : `u64` id du job ; libère l'état, sans effet si le job est inconnu. L'id 0 libère le job actif quel qu'il soit : le Worker l'utilise quand l'abandon survient pendant `Begin`, avant d'avoir lu l'id. Un `Begin` invalide est refusé sans toucher au job actif ; un `Begin` valide remplace le job précédent.
- `ATJB` : `u16 1`, `u16 état` (0 en cours, 1 annulé), `u32 fait`, `u32 total` (toujours 1000), `u64 id`. Le résultat final d'une ouverture est exactement la réponse `ATRP` de l'opcode 18 ; celui d'un export est `ATSV` (`u16 1`, `u16 0`, `u32 nombre d'avis`, octets DOCX).

Étapes. Ouverture : import du paquet, puis mise en page par lots de 8 paragraphes, peinture par lots de 2 048 lignes placées, encodage d'une page par étape (progression 100 à 1000). Export : import (300), écriture `write_docx` (700), relecture de vérification des octets écrits, puis résultat. Les jobs vivent hors de la session d'édition : ni une étape ni une annulation ne modifie le texte, la sélection ou l'historique. Les erreurs (`EDITOR_PREVIEW_FAILED`, `EDITOR_EXPORT_FAILED`, `EDITOR_JOB_UNKNOWN`) libèrent le job et se rendent en trame `Failed`.

Côté API : envoyer `encodeStagedBegin(requestId, StagedKind.OpenDocx | ExportDocx, docxBytes)` ([wasm-protocol.mjs](../web/wasm-protocol.mjs)) avec `client.request(frame, { signal, onProgress })`. `onProgress` reçoit en plus la phase `step` (avec `jobId`, BigInt) : `copiedBytes` est alors la progression sur l'échelle fixe `totalBytes = 1000`, monotone ; les phases `input` (copie de la trame `Begin`) et `output` (copie du résultat final) restent en octets. Le client contrôle la monotonie phase par phase. `decodeStagedExport(payload)` lit le résultat d'un export ; le résultat d'une ouverture se décode avec `decodeEditorResponse`. Après un abandon, le Worker envoie `Cancel` au moteur et la promesse rejette avec `AbortError` ; la commande suivante réussit.

Limites. Une étape reste un appel synchrone : l'annulation prend effet à la fin de l'étape en cours (une étape de mise en page de 8 paragraphes, l'import, l'écriture DOCX ou l'encodage d'une page). L'import du paquet ZIP/XML et `write_docx` (compression incluse) ne sont pas subdivisés ; leur durée borne la latence d'annulation sur de gros fichiers. La progression d'un export n'a que trois paliers. Chaque étape de mise en page et de peinture reconstruit le gestionnaire de polices (4 polices Carlito plus les polices embarquées), car il emprunte les octets de polices et ne peut pas vivre dans le job sans structure auto-référentielle : coût supplémentaire par étape, accepté. `paginate`, qui clôt la dernière étape de mise en page, n'est pas borné par le budget d'étapes. L'état d'un job (octets DOCX, modèle importé, mises en page) n'est pas compté dans `AllocationRegistry` (seules les trames le sont) ; il est borné par les limites d'import, et la mémoire linéaire WASM ne rétrécit pas après libération. La valeur 1000 n'est jamais émise en progression avant le résultat final (maximum 999 pour une ouverture, 700 pour un export). L'export couvre le DOCX importé tel quel (aucune édition riche n'est branchée dans l'ABI) ; l'enregistrement TXT (opcodes 10 et 11) est borné à 128 KiB et reste en un appel. Aucune chaîne d'interface n'a été ajoutée.

Preuves : tests Rust `staged::tests` (résultat découpé identique au one-shot, y compris avec des lots d'un paragraphe et de 7 placements, progression monotone, export déterministe et relisible, annulation, remplacement d'un job abandonné, entrées invalides) ; `tests/wasm-worker.mjs` (≥ 2 progressions, annulation `AbortError`, commande suivante réussie, résultat sans annulation identique à l'opcode 18, export idempotent) ; `tests/worker-browser.mjs` dans Edge headless (ouverture et export annulés puis rejoués).

Statut US-092 : transport et opérations longues (ouverture et export DOCX) vérifiés ; le quatrième critère est fermé au niveau API. Le [banc navigateur](../tests/worker-browser.html) est exécuté par `node tests/run-worker-browser.mjs` dans Edge headless en local et en CI, sans isolation SharedArrayBuffer. Il couvre le démarrage Worker ES par défaut, les octets/IDs, FIFO, annulation, reprise et l'ouverture/export annulables. Aucun bouton d'annulation n'est raccordé à l'interface (`web/app.mjs` hors périmètre) ; le shell hébergeant WebView2 n'a pas été exercé avec ces opérations.
