import { tauriLiveQueryClient, tauriTransactionClient } from "@hypr/db-tauri";

// One-shot `execute`/`executeTransaction` only -- live subscriptions moved to the
// index commands + `index-changed` event (see ~/shared/index-query). The remaining
// callers are Phase E3's to port.
export const liveQueryClient = tauriLiveQueryClient;
export const executeTransaction = tauriTransactionClient.executeTransaction;
