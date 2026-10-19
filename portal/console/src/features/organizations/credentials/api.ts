import { queryOptions } from "@tanstack/react-query";
import { api, AuthError } from "../../../lib/api.ts";
import { queryClient, querySession } from "../../../lib/query-client.ts";
import type { CurrentUser } from "../../auth/session.ts";
import { invalidateOrganization, organizationsQuery, refreshOrganizations } from "../api.ts";
import { organizationBucketsQuery } from "../buckets/api.ts";

export type CredentialAccessType = "ALL" | "RESTRICTED";

export interface Credential {
  organization_id: string;
  key_id: string;
  access_type: CredentialAccessType;
  bucket_names: string[];
  created_at: string;
}

export interface CreatedCredential extends Credential {
  secret: string;
}

export type CredentialInput =
  | { access_type: "ALL" }
  | { access_type: "RESTRICTED"; bucket_names: string[] };

function isKeyId(value: string) {
  return value.length === 20 && /^[A-Za-z0-9]+$/.test(value);
}

function isCredential(value: unknown, organizationId: string): value is Credential {
  return !!value && typeof value === "object"
    && "organization_id" in value && value.organization_id === organizationId
    && "key_id" in value && typeof value.key_id === "string" && isKeyId(value.key_id)
    && "access_type" in value && (value.access_type === "ALL" || value.access_type === "RESTRICTED")
    && "created_at" in value && typeof value.created_at === "string"
    && Number.isFinite(Date.parse(value.created_at))
    && "bucket_names" in value && Array.isArray(value.bucket_names)
    && value.bucket_names.every((name: unknown) => typeof name === "string")
    && new Set(value.bucket_names).size === value.bucket_names.length
    && (value.access_type !== "ALL" || value.bucket_names.length === 0);
}

function metadata(value: Credential): Credential {
  // Pick fields explicitly so no response secret can reach the query or mutation cache.
  return {
    organization_id: value.organization_id,
    key_id: value.key_id,
    access_type: value.access_type,
    bucket_names: [...value.bucket_names].sort(),
    created_at: value.created_at,
  };
}

function requestBody(input: CredentialInput): CredentialInput {
  if (input.access_type === "ALL") return { access_type: "ALL" };
  const bucketNames = [...input.bucket_names].sort();
  if (new Set(bucketNames).size !== bucketNames.length) {
    throw new AuthError("invalid_bucket_selection", 400);
  }
  return { access_type: "RESTRICTED", bucket_names: bucketNames };
}

function matchesInput(credential: Credential, input: CredentialInput) {
  if (credential.access_type !== input.access_type) return false;
  if (input.access_type === "ALL") return credential.bucket_names.length === 0;
  return credential.bucket_names.length === input.bucket_names.length
    && credential.bucket_names.every((name, index) => name === input.bucket_names[index]);
}

function requireSession(session: number) {
  if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
}

async function credentialsRequest(
  user: CurrentUser,
  organizationId: string,
  suffix = "",
  options: Parameters<typeof api>[1] = {},
): Promise<unknown> {
  const session = querySession();
  try {
    const response = await api(`organizations/${encodeURIComponent(organizationId)}/credentials${suffix}`, options);
    requireSession(session);
    const body: unknown = response.status === 204 ? undefined : await response.json().catch(() => null);
    requireSession(session);
    return body;
  } catch (error) {
    requireSession(session);
    if (error instanceof AuthError) {
      if (error.code === "owner_required" || error.code === "organization_not_found") {
        await refreshOrganizations(user).catch(() => {});
      } else if (error.code === "credential_not_found") {
        await queryClient.invalidateQueries({
          queryKey: organizationCredentialsQuery(user, organizationId).queryKey, exact: true,
        });
      } else if (error.code === "invalid_bucket_selection") {
        await queryClient.invalidateQueries({
          queryKey: organizationBucketsQuery(user, organizationId).queryKey, exact: true,
        });
      }
    }
    requireSession(session);
    throw error;
  } finally {
    if (options.method === "POST" || options.method === "DELETE") {
      // Refresh independently so counts cannot delay returning the one-time secret.
      void invalidateOrganization(user, organizationId, session).catch(() => {});
    }
  }
}

export function organizationCredentialsQuery(user: CurrentUser, organizationId: string) {
  return queryOptions({
    queryKey: [...organizationsQuery(user).queryKey, organizationId, "credentials"],
    queryFn: async ({ signal }): Promise<Credential[]> => {
      const body = await credentialsRequest(user, organizationId, "", { signal });
      if (!Array.isArray(body) || !body.every((item): item is Credential => isCredential(item, organizationId))
        || new Set(body.map((item) => item.key_id)).size !== body.length) {
        throw new AuthError("invalid_credentials_response", 502);
      }
      return body.map(metadata);
    },
    staleTime: 60_000,
    retry: (count, error) => count < 1 && error.name !== "AbortError"
      && !(error instanceof AuthError && error.status < 500),
  });
}

async function updateCredentials(
  user: CurrentUser,
  organizationId: string,
  session: number,
  update: (credentials: Credential[]) => Credential[],
) {
  requireSession(session);
  const { queryKey } = organizationCredentialsQuery(user, organizationId);
  await queryClient.cancelQueries({ queryKey, exact: true });
  requireSession(session);
  queryClient.setQueryData<Credential[]>(queryKey, (previous) => previous === undefined ? undefined : update(previous));
}

export async function createOrganizationCredential(
  user: CurrentUser,
  organizationId: string,
  input: CredentialInput,
  onCreated: (created: CreatedCredential) => void,
): Promise<Credential> {
  const session = querySession();
  const payload = requestBody(input);
  const body = await credentialsRequest(user, organizationId, "", { method: "POST", body: payload });
  requireSession(session);
  if (!isCredential(body, organizationId) || !("secret" in body) || typeof body.secret !== "string"
    || body.secret.length !== 40 || !/^[A-Za-z0-9]+$/.test(body.secret)) {
    throw new AuthError("invalid_credentials_response", 502);
  }
  const credential = metadata(body);
  if (!matchesInput(credential, payload)) throw new AuthError("invalid_credentials_response", 502);
  await updateCredentials(user, organizationId, session, (credentials) => [
    ...credentials.filter((item) => item.key_id !== credential.key_id), credential,
  ].sort((a, b) => a.key_id < b.key_id ? -1 : a.key_id > b.key_id ? 1 : 0));
  requireSession(session);
  // The caller keeps this only in local component state. Never return it from a mutation.
  onCreated({ ...credential, secret: body.secret });
  return credential;
}

export async function updateOrganizationCredentialAccess(
  user: CurrentUser,
  organizationId: string,
  keyId: string,
  input: CredentialInput,
): Promise<Credential> {
  const session = querySession();
  if (!isKeyId(keyId)) throw new AuthError("invalid_key_id", 400);
  const payload = requestBody(input);
  const body = await credentialsRequest(user, organizationId, `/${encodeURIComponent(keyId)}`, {
    method: "PATCH", body: payload,
  });
  requireSession(session);
  if (!isCredential(body, organizationId) || body.key_id !== keyId) {
    throw new AuthError("invalid_credentials_response", 502);
  }
  const credential = metadata(body);
  if (!matchesInput(credential, payload)) throw new AuthError("invalid_credentials_response", 502);
  await updateCredentials(user, organizationId, session,
    (credentials) => credentials.map((item) => item.key_id === keyId ? credential : item));
  requireSession(session);
  return credential;
}

export async function deleteOrganizationCredential(user: CurrentUser, organizationId: string, keyId: string) {
  const session = querySession();
  if (!isKeyId(keyId)) throw new AuthError("invalid_key_id", 400);
  await credentialsRequest(user, organizationId, `/${encodeURIComponent(keyId)}`, { method: "DELETE" });
  await updateCredentials(user, organizationId, session,
    (credentials) => credentials.filter((item) => item.key_id !== keyId));
  requireSession(session);
}

export function credentialErrorMessage(error: unknown) {
  if (error instanceof AuthError) {
    switch (error.code) {
      case "invalid_key_id": return "L'identifiant doit contenir exactement 20 caractères alphanumériques.";
      case "invalid_bucket_selection": return "Sélectionnez des buckets existants de cette organisation, sans doublons.";
      case "credential_not_found": return "Cette clé n'est plus disponible. La liste a été actualisée.";
      case "owner_required": return "Seul un propriétaire peut créer, modifier ou supprimer une clé d'accès.";
      case "organization_not_found": return "Cette organisation n'existe pas ou vous n'y avez plus accès.";
      case "email_not_verified": return "Une adresse email vérifiée est nécessaire pour consulter les clés d'accès.";
      case "invalid_credentials_response": return "Le service a renvoyé des données de clés invalides.";
      case "invalid_request": return "Vérifiez la portée et la sélection de buckets de la clé.";
      case "csrf_rejected": return "La requête a été refusée. Rechargez la page avant de réessayer.";
      case "payload_too_large": return "La sélection de buckets dépasse la taille autorisée.";
      case "key_generation_failed": return "La génération de la clé a échoué. Réessayez dans un instant.";
      case "database_error": return "Impossible d'enregistrer ou de charger les clés pour le moment.";
    }
    if (error.status === 401) return "Votre session a expiré. Reconnectez-vous pour continuer.";
  }
  return "Impossible de joindre le service des clés d'accès. Réessayez dans un instant.";
}
