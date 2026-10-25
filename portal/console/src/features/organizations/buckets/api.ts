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

export interface BucketWithUsage extends Bucket {
  stored_bytes: number;
  stored_objects: number;
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

function isBucketWithUsage(value: unknown, organizationId: string): value is BucketWithUsage {
  return isBucket(value, organizationId)
    && "stored_bytes" in value && typeof value.stored_bytes === "number"
    && Number.isSafeInteger(value.stored_bytes) && value.stored_bytes >= 0
    && "stored_objects" in value && typeof value.stored_objects === "number"
    && Number.isSafeInteger(value.stored_objects) && value.stored_objects >= 0;
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
    queryFn: async ({ signal }): Promise<BucketWithUsage[]> => {
      const session = querySession();
      const body = await bucketsRequest(user, organizationId, "", { signal });
      requireSession(session);
      if (!Array.isArray(body) || !body.every((item): item is BucketWithUsage => isBucketWithUsage(item, organizationId))
        || new Set(body.map((item) => item.name)).size !== body.length) {
        throw new AuthError("invalid_buckets_response", 502, session);
      }
      return body;
    },
    staleTime: 60_000,
    retry: (count, error) => count < 1 && error.name !== "AbortError"
      && !(error instanceof AuthError && error.status < 500),
  });
}

async function refreshBuckets(
  user: CurrentUser,
  organizationId: string,
  session: number,
  update?: (buckets: BucketWithUsage[]) => BucketWithUsage[],
) {
  requireSession(session);
  const { queryKey } = organizationBucketsQuery(user, organizationId);
  await queryClient.cancelQueries({ queryKey, exact: true });
  requireSession(session);
  if (update) {
    // Metadata-only writes must preserve the last measured usage in enriched rows.
    queryClient.setQueryData<BucketWithUsage[]>(queryKey, (previous) => previous === undefined ? undefined : update(previous));
  }
  await queryClient.invalidateQueries({ queryKey, exact: true });
  requireSession(session);
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
  // The POST response has no usage; fetch the full list instead of inventing it.
  await refreshBuckets(user, organizationId, session);
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
  await refreshBuckets(user, organizationId, session,
    (buckets) => buckets.map((item) => item.name === name ? { ...item, access: body.access } : item));
  requireSession(session);
  return body;
}

export function bucketErrorKey(error: unknown) {
  if (error instanceof AuthError) {
    switch (error.code) {
      case "invalid_bucket_name": return "utilisezUnNomDe3A63CaracteresConforme" as const;
      case "bucket_already_exists": return "ceNomDeBucketEstDejaUtiliseChoisissezUn" as const;
      case "bucket_not_found": return "ceBucketNEstPlusDisponibleLaListeA" as const;
      case "owner_required": return "seulUnProprietairePeutCreerUnBucketOuModifier" as const;
      case "organization_not_found": return "cetteOrganisationNExistePasOuVousNY2" as const;
      case "email_required": return "votreCompteDoitFournirUneAdresseEmailPourConsulter2" as const;
      case "email_not_verified": return "uneAdresseEmailVerifieeEstNecessairePourConsulterLes2" as const;
      case "invalid_buckets_response": return "leServiceARenvoyeDesDonneesDeBucketsInvalides" as const;
      case "invalid_request": return "verifiezLeNomEtLeNiveauDAccesDu" as const;
      case "csrf_rejected": return "laRequeteAEteRefuseeRechargezLaPageAvant" as const;
      case "payload_too_large": return "lesDonneesDuBucketDepassentLaTailleAutorisee" as const;
      case "database_error": return "impossibleDEnregistrerOuDeChargerLesBucketsPour" as const;
      case "metrics_unavailable": return "lesStatistiquesDesBucketsSontMomentanementIndisponibles" as const;
    }
    if (error.status === 401) return "votreSessionAExpireReconnectezVousPourContinuer" as const;
  }
  return "impossibleDeJoindreLeServiceDesBucketsReessayezDans" as const;
}
