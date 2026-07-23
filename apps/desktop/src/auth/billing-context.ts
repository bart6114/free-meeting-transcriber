import { createContext, useContext } from "react";

// Accounts/billing were removed (Task 4): every feature is unlocked locally.
// This stub keeps the `useBillingAccess()` surface that ~20 consumers
// destructure so none of them need editing.
export type BillingAccess = {
  isPro: boolean;
  isPaid: boolean;
  isTrialing: boolean;
  plan: "local";
  trialDaysRemaining: number | null;
  isReady: boolean;
  canStartTrial: { data: boolean; isPending: boolean };
  upgradeToPro: () => void;
  isUpgradingToPro: boolean;
};

export const BILLING: BillingAccess = {
  isPro: true,
  isPaid: true,
  isTrialing: false,
  plan: "local",
  trialDaysRemaining: null,
  isReady: true,
  canStartTrial: { data: false, isPending: false },
  upgradeToPro: () => {},
  isUpgradingToPro: false,
};

export const BillingContext = createContext<BillingAccess>(BILLING);

export function useBillingAccess() {
  return useContext(BillingContext);
}
