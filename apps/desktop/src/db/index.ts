import { createUseLiveQuery } from "@hypr/db-react";
import { tauriLiveQueryClient, tauriTransactionClient } from "@hypr/db-tauri";

export const liveQueryClient = tauriLiveQueryClient;
export const useLiveQuery = createUseLiveQuery(liveQueryClient);
export const executeTransaction = tauriTransactionClient.executeTransaction;
