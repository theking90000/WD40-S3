import { queryOptions } from "@tanstack/react-query";
import { api, AuthError } from "../../lib/api.ts";
import type { CurrentUser } from "../auth/session.ts";
import { queryClient, querySession } from "../../lib/query-client.ts";

export interface Organization {
  id: string;
  name: string;
  created_at: string;
  access: "MEMBER" | "OWNER";
}

export interface OrganizationCounts {
  buckets: number;
  credentials: number;
  members: number;
}

export interface OrganizationDetails extends Organization {
  counts: OrganizationCounts;
}

export function isOrganizationId(value: string) {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
}

function isOrganization(value: unknown): value is Organization {
  if (!value || typeof value !== "object") return false;
  return "id" in value && typeof value.id === "string" && isOrganizationId(value.id)
    && "name" in value && typeof value.name === "string"
    && "created_at" in value && typeof value.created_at === "string"
    && "access" in value && (value.access === "MEMBER" || value.access === "OWNER");
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isOrganizationCounts(value: unknown): value is OrganizationCounts {
  return !!value && typeof value === "object"
    && "buckets" in value && isCount(value.buckets)
    && "credentials" in value && isCount(value.credentials)
    && "members" in value && isCount(value.members);
}

export function organizationsQuery(user: CurrentUser) {
  return queryOptions({
    queryKey: ["organizations", import.meta.env?.BASE_URL ?? "/",
      user.issuer, user.subject, user.email, user.email_verified],
    queryFn: async ({ signal }): Promise<Organization[]> => {
      const response = await api("organizations", { signal });
      const body: unknown = await response.json();
      if (!Array.isArray(body) || !body.every(isOrganization)) {
        throw new AuthError("invalid_organizations_response", 502);
      }
      return body;
    },
    staleTime: 60_000,
    retry: (count, error) => count < 1 && !(error instanceof AuthError && error.status < 500),
  });
}

export function organizationQuery(user: CurrentUser, organizationId: string) {
  return queryOptions({
    queryKey: [...organizationsQuery(user).queryKey, organizationId, "details"],
    queryFn: async ({ signal }): Promise<OrganizationDetails> => {
      const session = querySession();
      try {
        const response = await api(`organizations/${encodeURIComponent(organizationId)}`, { signal });
        const body: unknown = await response.json();
        if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
        if (!isOrganization(body) || body.id !== organizationId
          || !("counts" in body) || !isOrganizationCounts(body.counts)) {
          throw new AuthError("invalid_organization_response", 502);
        }
        return { ...body, counts: body.counts };
      } catch (error) {
        if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
        if (error instanceof AuthError && error.code === "organization_not_found") {
          await refreshOrganizations(user).catch(() => {});
        }
        if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
        throw error;
      }
    },
    staleTime: 60_000,
    retry: (count, error) => count < 1 && error.name !== "AbortError"
      && !(error instanceof AuthError && error.status < 500),
  });
}

export async function invalidateOrganization(user: CurrentUser, organizationId: string, session: number) {
  if (session !== querySession()) return;
  const { queryKey } = organizationQuery(user, organizationId);
  // Cancel even an initial request so an older count cannot win after a write.
  await queryClient.cancelQueries({ queryKey, exact: true });
  if (session !== querySession()) return;
  await queryClient.invalidateQueries({ queryKey, exact: true });
}

export async function refreshOrganizations(user: CurrentUser) {
  const options = organizationsQuery(user);
  await queryClient.invalidateQueries({ queryKey: options.queryKey, exact: true, refetchType: "none" });
  return queryClient.fetchQuery(options);
}

export async function createOrganization(user: CurrentUser, name: string) {
  const currentSession = querySession();
  const trimmed = name.trim();
  if (!trimmed || Array.from(trimmed).length > 128) throw new AuthError("invalid_name", 400);
  const response = await api("organizations", { method: "POST", body: { name: trimmed } });
  const organization: unknown = await response.json();
  if (currentSession !== querySession()) throw new DOMException("Session changed", "AbortError");
  if (!isOrganization(organization)) throw new AuthError("invalid_organizations_response", 502);
  const options = organizationsQuery(user);
  // An older listing must not overwrite the newly created organization.
  await queryClient.cancelQueries({ queryKey: options.queryKey, exact: true });
  if (currentSession !== querySession()) throw new DOMException("Session changed", "AbortError");
  queryClient.setQueryData(options.queryKey, (previous = []) => [
    organization, ...previous.filter(({ id }) => id !== organization.id),
  ]);
  return organization;
}

export function organizationErrorMessage(error: unknown) {
  if (error instanceof AuthError) {
    if (error.code === "invalid_name") return "Le nom doit contenir entre 1 et 128 caractères.";
    if (error.code === "organization_name_taken") return "Une organisation porte déjà ce nom.";
    if (error.code === "email_required") return "Votre compte doit fournir une adresse email pour accéder aux organisations.";
    if (error.code === "email_not_verified") return "Une adresse email vérifiée est nécessaire pour accéder aux organisations.";
    if (error.status === 401) return "Votre session a expiré. Reconnectez-vous pour continuer.";
  }
  return "Impossible de joindre le service des organisations. Réessayez dans un instant.";
}
