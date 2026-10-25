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

function requireSession(session: number) {
  if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
}

async function organizationWrite(
  user: CurrentUser,
  organizationId: string,
  method: "PATCH" | "DELETE",
  body?: { name: string },
) {
  const session = querySession();
  try {
    const response = await api(`organizations/${encodeURIComponent(organizationId)}`, { method, body });
    requireSession(session);
    if (response.status !== (method === "DELETE" ? 204 : 200)) {
      throw new AuthError("invalid_organization_response", 502);
    }
    const result: unknown = method === "DELETE" ? undefined : await response.json().catch(() => null);
    requireSession(session);
    return { body: result, session };
  } catch (error) {
    requireSession(session);
    if (error instanceof AuthError) {
      if (error.code === "owner_required" || error.code === "organization_not_found") {
        await refreshOrganizations(user).catch(() => {});
      } else if (error.code === "organization_not_empty") {
        await invalidateOrganization(user, organizationId, session).catch(() => {});
      }
    }
    requireSession(session);
    throw error;
  }
}

export async function updateOrganization(user: CurrentUser, organizationId: string, name: string) {
  const trimmed = name.trim();
  if (!trimmed || Array.from(trimmed).length > 128) throw new AuthError("invalid_name", 400);
  const { body, session } = await organizationWrite(user, organizationId, "PATCH", { name: trimmed });
  requireSession(session);
  if (!isOrganization(body) || body.id !== organizationId || body.name !== trimmed) {
    throw new AuthError("invalid_organization_response", 502);
  }
  const listKey = organizationsQuery(user).queryKey;
  const detailsKey = organizationQuery(user, organizationId).queryKey;
  await Promise.all([
    queryClient.cancelQueries({ queryKey: listKey, exact: true }),
    queryClient.cancelQueries({ queryKey: detailsKey, exact: true }),
  ]);
  requireSession(session);
  queryClient.setQueryData<Organization[]>(listKey, (previous) => previous?.map((item) => item.id === organizationId ? body : item));
  queryClient.setQueryData<OrganizationDetails>(detailsKey, (previous) => previous ? { ...previous, ...body } : undefined);
  return body;
}

export async function deleteOrganization(user: CurrentUser, organizationId: string) {
  const { session } = await organizationWrite(user, organizationId, "DELETE");
  requireSession(session);
  const listKey = organizationsQuery(user).queryKey;
  const organizationKey = [...listKey, organizationId];
  await Promise.all([
    queryClient.cancelQueries({ queryKey: listKey, exact: true }),
    queryClient.cancelQueries({ queryKey: organizationKey }),
  ]);
  requireSession(session);
  queryClient.removeQueries({ queryKey: organizationKey });
  queryClient.setQueryData<Organization[]>(listKey, (previous) => previous?.filter(({ id }) => id !== organizationId));
}

export function organizationErrorKey(error: unknown) {
  if (error instanceof AuthError) {
    if (error.code === "invalid_name") return "leNomDoitContenirEntre1Et128Caracteres" as const;
    if (error.code === "organization_name_taken") return "uneOrganisationPorteDejaCeNom" as const;
    if (error.code === "organization_not_empty") return "cetteOrganisationContientEncoreDesBucketsOuDesCles" as const;
    if (error.code === "owner_required") return "seulUnProprietairePeutModifierOuSupprimerCetteOrganisation" as const;
    if (error.code === "organization_not_found") return "cetteOrganisationNExistePlusOuVousNY" as const;
    if (error.code === "csrf_rejected") return "laRequeteAEteRefuseeRechargezLaPageEt" as const;
    if (error.code === "email_required") return "votreCompteDoitFournirUneAdresseEmailPourAcceder" as const;
    if (error.code === "email_not_verified") return "uneAdresseEmailVerifieeEstNecessairePourAccederAux" as const;
    if (error.status === 401) return "votreSessionAExpireReconnectezVousPourContinuer" as const;
  }
  return "impossibleDeJoindreLeServiceDesOrganisationsReessayezDans" as const;
}
