import { queryOptions } from "@tanstack/react-query";
import { getCurrentUser } from "./auth.ts";
import type { CurrentUser } from "./auth.ts";
import { currentUserKey, queryClient, querySession, setQueryUser } from "./query-client.ts";

export function currentUserQuery() {
  return queryOptions({
    queryKey: currentUserKey,
    queryFn: ({ signal }) => getCurrentUser({ signal }),
    staleTime: 60_000,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

export async function getSessionUser(): Promise<CurrentUser | null> {
  const session = querySession();
  // The shared query owns its signal; cancelling one navigation must not cancel
  // another navigation's authentication request.
  const user = await queryClient.fetchQuery(currentUserQuery());
  const cached = queryClient.getQueryData<CurrentUser | null>(currentUserKey);
  // Another consumer may already have reconciled this same completed query.
  // A logout or callback replaces or removes it, so an old result is discarded.
  if (session !== querySession() && cached !== user) {
    if (cached === null) return null;
    throw new DOMException("Session changed", "AbortError");
  }
  setQueryUser(user);
  return cached === undefined ? user : cached;
}
