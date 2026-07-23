import { createContext, useContext } from "react";

// Accounts/billing were removed (Task 4): the app is fully local and always
// runs signed-out. This stub keeps the `useAuth()` surface that ~15
// consumers destructure so none of them need editing.
export type StubSession = {
  user: { id: string; email?: string };
  access_token: string;
};

export type AuthContextType = {
  // undefined = initial load in progress, null = known unauthenticated.
  // Always null here: there is no session to load.
  session: StubSession | null;
  isRefreshingSession: boolean;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
  refreshSession: () => Promise<StubSession | null>;
  handleAuthCallback: (url: string) => Promise<void>;
  setSessionFromTokens: (
    accessToken: string,
    refreshToken: string,
  ) => Promise<void>;
  getHeaders: () => Record<string, string> | null;
};

export const AUTH: AuthContextType = {
  session: null,
  isRefreshingSession: false,
  signIn: async () => {},
  signOut: async () => {},
  refreshSession: async () => null,
  handleAuthCallback: async () => {},
  setSessionFromTokens: async () => {},
  getHeaders: () => null,
};

export const AuthContext = createContext<AuthContextType>(AUTH);

export function useOptionalAuth() {
  return useContext(AuthContext);
}

export function useAuth() {
  return useContext(AuthContext);
}
