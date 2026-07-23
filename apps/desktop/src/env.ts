import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const env = createEnv({
  clientPrefix: "VITE_",
  client: {
    VITE_APP_VERSION: z.string().min(1).optional(),
  },
  runtimeEnv: import.meta.env,
  emptyStringAsUndefined: true,
});

// This app's original desktop build pointed two removed env vars at hosted
// backend infrastructure. Accounts/billing (and the server code they talked
// to) were removed in Task 4 — the "hyprnote" hosted AI provider and
// OAuth-integration code paths that referenced them are permanently
// unreachable now (auth is always signed-out, billing is always local), but
// are left in place pending a later gating cleanup. These constants are the
// same localhost defaults those env vars used to fall back to, kept only so
// that dead code still type-checks.
export const HOSTED_API_URL = "http://localhost:3001";
export const HOSTED_APP_URL = "http://localhost:3000";
