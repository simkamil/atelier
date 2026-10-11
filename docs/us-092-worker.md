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

Le client traite uniquement le transport et l’écho ABI de US-090. La réponse n’est pas un événement métier `Completed`; les handlers document et rendu restent à US-120. La validation Node ci-dessous exerce les modules de production et le véritable artefact WASM dans `worker_threads`. Elle ne valide ni clavier/IME ni WebView2.

## Vérification

```sh
node tests/wasm-worker.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm
node tests/wasm-abi.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm target/wasm-abi-hooks/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm
```

Le test Worker vérifie les IDs u64 au-delà de `2^53`, l’ordre FIFO, les sous-vues, la copie progressive, l’annulation après une progression partielle, la reprise, la fermeture et le détachement observé après le vrai `postMessage` avec transfert.

Statut US-092 : transport vérifié, US encore ouverte. Le dispatch WASM reste un écho ; aucune ouverture/export documentaire n’est validée. Les copies par blocs progressent et s’annulent réellement, mais les handlers métier doivent encore offrir des points de coopération avant de clôturer le quatrième critère. Le [banc navigateur](../tests/worker-browser.html) est exécuté par `node tests/run-worker-browser.mjs` dans Edge headless en local et en CI, sans isolation SharedArrayBuffer. Il couvre le démarrage Worker ES par défaut, les octets/IDs, FIFO, annulation et reprise. Le shell héberge maintenant WebView2 ; les ressources de l’application et le pont natif restent à intégrer.
