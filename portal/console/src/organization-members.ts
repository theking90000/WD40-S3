import { queryOptions } from "@tanstack/react-query";
import { api, AuthError } from "./auth.ts";
import type { CurrentUser } from "./auth.ts";
import { organizationsQuery, refreshOrganizations } from "./organizations.ts";
import { queryClient, querySession } from "./query-client.ts";

export interface OrganizationMember {
  email: string;
  access: "MEMBER" | "OWNER";
  created_at: string;
}

function isMember(value: unknown): value is OrganizationMember {
  return !!value && typeof value === "object"
    && "email" in value && typeof value.email === "string"
    && "access" in value && (value.access === "MEMBER" || value.access === "OWNER")
    && "created_at" in value && typeof value.created_at === "string"
    && Number.isFinite(Date.parse(value.created_at));
}

async function membersRequest(
  user: CurrentUser,
  organizationId: string,
  suffix = "",
  options: Parameters<typeof api>[1] = {},
) {
  const session = querySession();
  try {
    const response = await api(`organizations/${organizationId}/members${suffix}`, options);
    if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
    return response;
  } catch (error) {
    if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
    if (error instanceof AuthError) {
      if (error.code === "owner_required" || error.code === "organization_not_found") {
        // Preserve the original error if refreshing permissions also fails.
        await refreshOrganizations(user).catch(() => {});
      } else if (error.code === "member_already_exists" || error.code === "member_not_found") {
        await queryClient.invalidateQueries({
          queryKey: organizationMembersQuery(user, organizationId).queryKey, exact: true,
        });
      }
    }
    throw error;
  }
}

export function organizationMembersQuery(user: CurrentUser, organizationId: string) {
  return queryOptions({
    queryKey: [...organizationsQuery(user).queryKey, organizationId, "members"],
    queryFn: async ({ signal }): Promise<OrganizationMember[]> => {
      const response = await membersRequest(user, organizationId, "", { signal });
      const body: unknown = await response.json();
      if (!Array.isArray(body) || !body.every(isMember)) {
        throw new AuthError("invalid_members_response", 502);
      }
      return body;
    },
    staleTime: 60_000,
    retry: (count, error) => count < 1 && !(error instanceof AuthError && error.status < 500),
  });
}

async function updateMembers(
  user: CurrentUser,
  organizationId: string,
  session: number,
  update: (members: OrganizationMember[]) => OrganizationMember[],
) {
  if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
  const { queryKey } = organizationMembersQuery(user, organizationId);
  await queryClient.cancelQueries({ queryKey, exact: true });
  if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
  queryClient.setQueryData<OrganizationMember[]>(queryKey, (previous) => previous === undefined ? undefined : update(previous));
}

export async function addOrganizationMember(user: CurrentUser, organizationId: string, email: string) {
  const session = querySession();
  const response = await membersRequest(user, organizationId, "", {
    method: "POST", body: { email: email.trim() },
  });
  const member: unknown = await response.json();
  if (!isMember(member)) throw new AuthError("invalid_members_response", 502);
  await updateMembers(user, organizationId, session, (members) => [
    ...members.filter((item) => item.email !== member.email), member,
  ].sort((a, b) => a.email < b.email ? -1 : a.email > b.email ? 1 : 0));
  return member;
}

export async function removeOrganizationMember(user: CurrentUser, organizationId: string, email: string) {
  const session = querySession();
  await membersRequest(user, organizationId, `/${encodeURIComponent(email)}`, { method: "DELETE" });
  await updateMembers(user, organizationId, session, (members) => members.filter((member) => member.email !== email));
}

export async function updateOrganizationMemberAccess(
  user: CurrentUser,
  organizationId: string,
  email: string,
  access: OrganizationMember["access"],
) {
  const session = querySession();
  const response = await membersRequest(user, organizationId, `/${encodeURIComponent(email)}`, {
    method: "PATCH", body: { access },
  });
  const member: unknown = await response.json();
  if (!isMember(member) || member.email !== email) throw new AuthError("invalid_members_response", 502);
  await updateMembers(user, organizationId, session, (members) => members.map((item) => item.email === email ? member : item));
  return member;
}

export function memberErrorMessage(error: unknown) {
  if (error instanceof AuthError) {
    switch (error.code) {
      case "invalid_email": return "Saisissez une adresse email valide de 254 octets maximum.";
      case "member_already_exists": return "Cette adresse email est déjà membre de l'organisation.";
      case "member_not_found": return "Ce membre a déjà été retiré. La liste a été actualisée.";
      case "owner_removal_forbidden": return "Un propriétaire ne peut pas être retiré de l'organisation.";
      case "owner_required": return "Seul un propriétaire peut ajouter ou retirer des membres et modifier leur rôle.";
      case "self_update_forbidden": return "Vous ne pouvez pas modifier votre propre rôle.";
      case "organization_not_found": return "Cette organisation n'existe pas ou vous n'y avez plus accès.";
      case "email_not_verified": return "Une adresse email vérifiée est nécessaire pour consulter les membres.";
    }
    if (error.status === 401) return "Votre session a expiré. Reconnectez-vous pour continuer.";
  }
  return "Impossible de joindre le service des membres. Réessayez dans un instant.";
}
