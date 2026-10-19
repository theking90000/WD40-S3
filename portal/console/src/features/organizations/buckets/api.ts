import { queryOptions } from "@tanstack/react-query";
import { api, AuthError } from "../../../lib/api.ts";
import { queryClient, querySession } from "../../../lib/query-client.ts";
import type { CurrentUser } from "../../auth/session.ts";
import { invalidateOrganization, organizationsQuery, refreshOrganizations } from "../api.ts";

export type BucketAccess = "PRIVATE" | "PUBLIC" | "PUBLIC_LIST";

export interface Bucket {
  organization_id: string;
  name: string;
  access: BucketAccess;
  created_at: string;
}

export interface CreateBucketInput {
  name: string;
  access: BucketAccess;
}

export function isValidBucketName(name: string) {
  const parts = name.split(".");
  const ipAddress = parts.length === 4
    && parts.every((part) => /^[0-9]+$/.test(part) && Number(part) <= 255);
  return name.length >= 3 && name.length <= 63
    && !/[^a-z0-9.-]/.test(name)
    && /^[a-z0-9]/.test(name) && /[a-z0-9]$/.test(name)
    && !name.includes("..") && !ipAddress
    && !["xn--", "sthree-", "amzn-s3-demo-"].some((prefix) => name.startsWith(prefix))
    && !["-s3alias", "--ol-s3", ".mrap", "--x-s3", "--table-s3", "-an"]
      .some((suffix) => name.endsWith(suffix));
}

function isBucket(value: unknown, organizationId: string): value is Bucket {
  return !!value && typeof value === "object"
    && "organization_id" in value && value.organization_id === organizationId
    && "name" in value && typeof value.name === "string"
    && "access" in value && (value.access === "PRIVATE" || value.access === "PUBLIC" || value.access === "PUBLIC_LIST")
    && "created_at" in value && typeof value.created_at === "string"
    && Number.isFinite(Date.parse(value.created_at));
}

function requireSession(session: number) {
  if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
}

async function bucketsRequest(
  user: CurrentUser,
  organizationId: string,
  suffix = "",
  options: Parameters<typeof api>[1] = {},
): Promise<unknown> {
  const session = querySession();
  try {
    const response = await api(`organizations/${encodeURIComponent(organizationId)}/buckets${suffix}`, options);
    requireSession(session);
    const body: unknown = await response.json().catch(() => null);
    requireSession(session);
    return body;
  } catch (error) {
    requireSession(session);
    if (error instanceof AuthError) {
      if (error.code === "owner_required" || error.code === "organization_not_found") {
        await refreshOrganizations(user).catch(() => {});
      } else if (error.code === "bucket_already_exists" || error.code === "bucket_not_found") {
        await queryClient.invalidateQueries({
          queryKey: organizationBucketsQuery(user, organizationId).queryKey, exact: true,
        });
      }
    }
    requireSession(session);
    throw error;
  } finally {
    if (options.method === "POST" || options.method === "DELETE") {
      // A failed synchronization can still leave a committed SQL change.
      void invalidateOrganization(user, organizationId, session).catch(() => {});
    }
  }
}

export function organizationBucketsQuery(user: CurrentUser, organizationId: string) {
  return queryOptions({
    queryKey: [...organizationsQuery(user).queryKey, organizationId, "buckets"],
    queryFn: async ({ signal }): Promise<Bucket[]> => {
      const body = await bucketsRequest(user, organizationId, "", { signal });
      if (!Array.isArray(body) || !body.every((item): item is Bucket => isBucket(item, organizationId))
        || new Set(body.map((item) => item.name)).size !== body.length) {
        throw new AuthError("invalid_buckets_response", 502);
      }
      return body;
    },
    staleTime: 60_000,
    retry: (count, error) => count < 1 && error.name !== "AbortError"
      && !(error instanceof AuthError && error.status < 500),
  });
}

async function updateBuckets(
  user: CurrentUser,
  organizationId: string,
  session: number,
  update: (buckets: Bucket[]) => Bucket[],
) {
  requireSession(session);
  const { queryKey } = organizationBucketsQuery(user, organizationId);
  await queryClient.cancelQueries({ queryKey, exact: true });
  requireSession(session);
  // An absent list must still be fetched in full, rather than seeded with one row.
  queryClient.setQueryData<Bucket[]>(queryKey, (previous) => previous === undefined ? undefined : update(previous));
}

export async function createOrganizationBucket(user: CurrentUser, organizationId: string, input: CreateBucketInput) {
  const session = querySession();
  if (!isValidBucketName(input.name)) throw new AuthError("invalid_bucket_name", 400);
  const body = await bucketsRequest(user, organizationId, "", {
    method: "POST", body: { name: input.name, access: input.access },
  });
  requireSession(session);
  if (!isBucket(body, organizationId) || body.name !== input.name || body.access !== input.access) {
    throw new AuthError("invalid_buckets_response", 502);
  }
  await updateBuckets(user, organizationId, session, (buckets) => [
    ...buckets.filter((item) => item.name !== body.name), body,
  ].sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  requireSession(session);
  return body;
}

export async function updateOrganizationBucketAccess(
  user: CurrentUser,
  organizationId: string,
  name: string,
  access: BucketAccess,
) {
  const session = querySession();
  const body = await bucketsRequest(user, organizationId, `/${encodeURIComponent(name)}`, {
    method: "PATCH", body: { access },
  });
  requireSession(session);
  if (!isBucket(body, organizationId) || body.name !== name || body.access !== access) {
    throw new AuthError("invalid_buckets_response", 502);
  }
  await updateBuckets(user, organizationId, session,
    (buckets) => buckets.map((item) => item.name === name ? body : item));
  requireSession(session);
  return body;
}

export function bucketErrorMessage(error: unknown) {
  if (error instanceof AuthError) {
    switch (error.code) {
      case "invalid_bucket_name": return "Utilisez un nom de 3 à 63 caractères conforme aux règles de nommage S3.";
      case "bucket_already_exists": return "Ce nom de bucket est déjà utilisé. Choisissez un autre nom.";
      case "bucket_not_found": return "Ce bucket n'est plus disponible. La liste a été actualisée.";
      case "owner_required": return "Seul un propriétaire peut créer un bucket ou modifier son accès.";
      case "organization_not_found": return "Cette organisation n'existe pas ou vous n'y avez plus accès.";
      case "email_not_verified": return "Une adresse email vérifiée est nécessaire pour consulter les buckets.";
      case "invalid_buckets_response": return "Le service a renvoyé des données de buckets invalides.";
      case "invalid_request": return "Vérifiez le nom et le niveau d'accès du bucket.";
      case "csrf_rejected": return "La requête a été refusée. Rechargez la page avant de réessayer.";
      case "payload_too_large": return "Les données du bucket dépassent la taille autorisée.";
      case "database_error": return "Impossible d'enregistrer ou de charger les buckets pour le moment.";
    }
    if (error.status === 401) return "Votre session a expiré. Reconnectez-vous pour continuer.";
  }
  return "Impossible de joindre le service des buckets. Réessayez dans un instant.";
}
