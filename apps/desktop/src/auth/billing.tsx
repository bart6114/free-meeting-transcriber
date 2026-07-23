import type { ReactNode } from "react";

import { BILLING, BillingContext } from "./billing-context";

// Accounts/billing were removed (Task 4). This provider just hands out the
// always-unlocked BILLING stub defined in ./billing-context — kept as a
// component (rather than inlining the constant) so `~/auth/billing`'s
// BillingProvider import keeps resolving for main-app-layout.tsx.
export function BillingProvider({ children }: { children: ReactNode }) {
  return (
    <BillingContext.Provider value={BILLING}>
      {children}
    </BillingContext.Provider>
  );
}
