// SPDX-License-Identifier: AGPL-3.0-or-later
import { parentPort, workerData } from "node:worker_threads";
import { installEngineWorker } from "../web/engine-worker.mjs";

const observer = workerData?.observer;
let crashScheduled = false;
const scope = {
  addEventListener(type, listener) {
    if (type === "message") parentPort.on("message", (data) => {
      listener({ data });
      if (workerData?.fatalOnRequest && data?.kind === "request" && !crashScheduled) {
        crashScheduled = true;
        setTimeout(() => { throw new Error("intentional worker fatal for lifecycle test"); }, 0);
      }
    });
  },
  removeEventListener() {},
  postMessage(message, transfer) {
    parentPort.postMessage(message, transfer);
    if (message?.kind === "result" && observer) {
      observer.postMessage({ kind: "transfer-observed", detached: message.bytes.buffer.byteLength === 0 });
    }
  },
};

installEngineWorker(scope);
