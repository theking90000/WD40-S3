import { MutationCache, QueryCache, QueryClient } from "@tanstack/react-query";
import type { CurrentUser } from "../features/auth/session.ts";

export const currentUserKey = ["current-user", import.meta.env?.BASE_URL ?? "/"] as const;

export const queryClient = new QueryClient({
  queryCache: new QueryCache({ onError: expireUnauthorizedSession }),
  mutationCache: new MutationCache({ onError: expireUnauthorizedSession }),
});

let identity: string | null = null;
let session = 0;

export function setQueryUser(user: CurrentUser | null) {
  const next = user ? JSON.stringify([
    import.meta.env?.BASE_URL ?? "/", user.issuer, user.subject, user.email, user.email_verified,
  ]) : null;
  if (next !== identity) {
    session += 1;
    identity = next;
    // The completed /me query belongs to the new identity. Keep it while
    // cancelling and discarding data and mutations from the previous identity.
    queryClient.removeQueries({ predicate: (query) => query.queryKey[0] !== currentUserKey[0] });
    queryClient.getMutationCache().clear();
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

export function expireQuerySession() {
  session += 1;
  identity = null;
  void queryClient.cancelQueries({ queryKey: currentUserKey, exact: true });
  queryClient.removeQueries({ predicate: (query) => query.queryKey[0] !== currentUserKey[0] });
  queryClient.getMutationCache().clear();
  // Preserve the user query's observers so the session layout immediately hides
  // protected pages and revalidates against this anonymous result.
  queryClient.setQueryData<CurrentUser | null>(currentUserKey, null);
}

function expireUnauthorizedSession(error: unknown) {
  if (error instanceof Error && "status" in error && error.status === 401
    && "session" in error && error.session === session) {
    // Run after the query/mutation fails so cancelling other requests does not
    // replace this request's authentication error with a cancellation error.
    expireQuerySession();
  }
}
