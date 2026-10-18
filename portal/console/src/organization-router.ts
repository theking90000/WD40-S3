import { useQuery } from "@tanstack/react-query";
import { createContext, replace, useParams, useRouteLoaderData } from "react-router";
import type { LoaderFunctionArgs, MiddlewareFunction } from "react-router";
import { AuthError } from "./auth.ts";
import type { CurrentUser } from "./auth.ts";
import { requireUser, withUser } from "./auth-router.ts";
import { isOrganizationId, organizationsQuery, refreshOrganizations } from "./organizations.ts";
import { queryClient } from "./query-client.ts";

const organizationContext = createContext<string>();

export function consolePath(path = "") {
  return `${import.meta.env?.BASE_URL ?? "/"}${path}`;
}

async function loadOrganizations(args: LoaderFunctionArgs, user: CurrentUser) {
  if (!user.email?.trim() || user.email_verified !== true) {
    throw new AuthError("email_not_verified", 403);
  }
  try {
    return await queryClient.fetchQuery(organizationsQuery(user));
  } catch (error) {
    if (error instanceof AuthError && error.status === 401) {
      const login = new URL(consolePath("login"), args.request.url);
      login.searchParams.set("returnTo", args.request.url);
      throw replace(login.href);
    }
    throw error;
  }
}

export const selectionLoader = withUser(async (args) => {
  const organizations = await loadOrganizations(args, args.user);
  const organization = organizations.at(0);
  if (organizations.length === 1 && organization
    && !new URL(args.request.url).searchParams.has("organization_unavailable")) {
    throw replace(consolePath(`${organization.id}/`));
  }
  return null;
});

export const organizationMiddleware: MiddlewareFunction = async (args, next) => {
  const user = await requireUser(args);
  const id = args.params.organizationId;
  if (!id || !isOrganizationId(id)) throw replace(consolePath("?organization_unavailable=1"));
  const cached = queryClient.getQueryState(organizationsQuery(user).queryKey);
  const wasFresh = cached?.data !== undefined && !cached.isInvalidated
    && Date.now() - cached.dataUpdatedAt < 60_000;
  let organizations = await loadOrganizations(args, user);
  if (!organizations.some((organization) => organization.id === id.toLowerCase())
    && wasFresh) {
    organizations = await refreshOrganizations(user);
  }
  if (!organizations.some((organization) => organization.id === id.toLowerCase())) {
    throw replace(consolePath("?organization_unavailable=1"));
  }
  args.context.set(organizationContext, id.toLowerCase());
  return next();
};

export const organizationLoader = withUser(({ context }) => ({
  organizationId: context.get(organizationContext),
}));

export function useOrganizations() {
  const data = useRouteLoaderData<{ user: CurrentUser }>("layouts/session");
  if (!data) throw new Error("Organizations require the authenticated layout");
  return { user: data.user, ...useQuery(organizationsQuery(data.user)) };
}

export function useOrganization() {
  const { organizationId } = useParams();
  const { data } = useOrganizations();
  return data?.find(({ id }) => id === organizationId?.toLowerCase());
}
