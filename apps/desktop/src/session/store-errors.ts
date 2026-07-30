// The session store's CAS guards reject with a stable "conflict:" prefix (see
// `StoreError::Conflict` in session_store/mod.rs). Callers use this to tell a benign
// compare-and-swap miss apart from a real failure across the IPC string boundary.
export function isStoreConflictError(error: string): boolean {
  return error.startsWith("conflict:");
}
