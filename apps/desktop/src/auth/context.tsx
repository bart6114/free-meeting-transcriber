import type { ReactNode } from "react";

import { AUTH, AuthContext } from "./auth-context";

// Accounts/billing were removed (Task 4). This provider just hands out the
// always-signed-out AUTH stub defined in ./auth-context — kept as a
// component (rather than inlining the constant) so `~/auth`'s AuthProvider
// import keeps resolving for main-app-layout.tsx.
export function AuthProvider({ children }: { children: ReactNode }) {
  return <AuthContext.Provider value={AUTH}>{children}</AuthContext.Provider>;
}
