import { QueryClient } from "@tanstack/react-query";
import type { CurrentUser } from "./auth.ts";

export const queryClient = new QueryClient();

let identity: string | null = null;
let session = 0;

export function setQueryUser(user: CurrentUser | null) {
  const next = user ? JSON.stringify([
    import.meta.env?.BASE_URL ?? "/", user.issuer, user.subject, user.email, user.email_verified,
  ]) : null;
  if (next !== identity) {
    resetQuerySession();
    identity = next;
  }
}

export function resetQuerySession() {
  session += 1;
  identity = null;
  // Removing queries also cancels them and ignores their late results.
  queryClient.clear();
}

export function querySession() {
  return session;
}
