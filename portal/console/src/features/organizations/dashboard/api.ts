import { queryOptions } from "@tanstack/react-query";
import { api, AuthError } from "../../../lib/api.ts";
import { querySession } from "../../../lib/query-client.ts";
import type { CurrentUser } from "../../auth/session.ts";
import { organizationErrorMessage, organizationsQuery, refreshOrganizations } from "../api.ts";

export interface DashboardPeriod {
  from: number;
  to: number;
  resolution_seconds: number;
}

export interface BucketUsage {
  name: string;
  stored_bytes: number;
  stored_objects: number;
}

export interface OrganizationDashboard {
  generated_at: number;
  period: DashboardPeriod;
  stored_bytes: number;
  stored_objects: number;
  requests: number;
  uploaded_bytes: number;
  downloaded_bytes: number;
  transferred_bytes: number;
  buckets: BucketUsage[];
}

export type UploadPeriod = "7d" | "30d";

export interface UploadPoint {
  start: number;
  uploaded_bytes: number;
  complete: boolean;
}

export interface OrganizationUploads {
  generated_at: number;
  period: DashboardPeriod;
  total_uploaded_bytes: number;
  points: UploadPoint[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isTimestamp(value: unknown): value is number {
  return isCount(value) && Number.isFinite(new Date(value * 1000).getTime());
}

function isBucketUsage(value: unknown): value is BucketUsage {
  return isRecord(value) && typeof value.name === "string" && value.name.length > 0
    && isCount(value.stored_bytes) && isCount(value.stored_objects);
}

function isDashboard(value: unknown): value is OrganizationDashboard {
  if (!isRecord(value) || !isRecord(value.period)) return false;
  const period = value.period;
  return isTimestamp(value.generated_at)
    && isTimestamp(period.from) && isTimestamp(period.to)
    && period.from < period.to && period.to <= value.generated_at
    && isCount(period.resolution_seconds) && period.resolution_seconds > 0
    && ["stored_bytes", "stored_objects", "requests", "uploaded_bytes", "downloaded_bytes", "transferred_bytes"]
      .every((field) => isCount(value[field]))
    && Array.isArray(value.buckets) && value.buckets.every(isBucketUsage)
    && new Set(value.buckets.map((bucket) => bucket.name)).size === value.buckets.length;
}

function isUploads(value: unknown, selectedPeriod: UploadPeriod): value is OrganizationUploads {
  if (!isRecord(value) || !isRecord(value.period)) return false;
  const { from, to, resolution_seconds } = value.period;
  const days = selectedPeriod === "7d" ? 7 : 30;
  const day = 86_400;
  return isTimestamp(value.generated_at)
    && isTimestamp(from) && isTimestamp(to)
    && from < to && to === value.generated_at
    && resolution_seconds === day && from % day === 0
    && from + (days - 1) * day === Math.floor(value.generated_at / day) * day
    && isCount(value.total_uploaded_bytes)
    && Array.isArray(value.points) && value.points.length === days
    && value.points.every((point: unknown, index: number) => isRecord(point)
      && isTimestamp(point.start) && point.start === from + index * day
      && isCount(point.uploaded_bytes) && typeof point.complete === "boolean"
      && (index === days - 1 || point.complete));
}

function requireSession(session: number) {
  if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
}

async function readDashboardResponse<T>(
  user: CurrentUser,
  path: string,
  signal: AbortSignal,
  validate: (body: unknown) => body is T,
): Promise<T> {
  const session = querySession();
  try {
    const response = await api(path, { signal });
    requireSession(session);
    const body: unknown = await response.json().catch((error: unknown) => {
      if (error instanceof SyntaxError) return null;
      throw error;
    });
    requireSession(session);
    if (!validate(body)) throw new AuthError("invalid_dashboard_response", 502, session);
    return body;
  } catch (error) {
    requireSession(session);
    if (error instanceof AuthError && error.code === "organization_not_found") {
      await refreshOrganizations(user).catch(() => {});
    }
    requireSession(session);
    throw error;
  }
}

export function organizationDashboardQuery(user: CurrentUser, organizationId: string) {
  return queryOptions({
    queryKey: [...organizationsQuery(user).queryKey, organizationId, "dashboard"],
    queryFn: ({ signal }) => readDashboardResponse(user,
      `organizations/${encodeURIComponent(organizationId)}/dashboard`, signal, isDashboard),
    staleTime: 30_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
    retry: (count, error) => count < 1 && error.name !== "AbortError"
      && !(error instanceof AuthError && error.status < 500),
  });
}

export function organizationUploadsQuery(user: CurrentUser, organizationId: string, period: UploadPeriod) {
  return queryOptions({
    queryKey: [...organizationsQuery(user).queryKey, organizationId, "dashboard", "uploads", period],
    queryFn: ({ signal }) => readDashboardResponse(user,
      `organizations/${encodeURIComponent(organizationId)}/dashboard/uploads?period=${period}`, signal,
      (body): body is OrganizationUploads => isUploads(body, period)),
    staleTime: 30_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
    retry: (count, error) => count < 1 && error.name !== "AbortError"
      && !(error instanceof AuthError && error.status < 500),
  });
}

export function dashboardErrorMessage(error: unknown) {
  if (error instanceof AuthError) {
    if (error.code === "metrics_unavailable") return "Les statistiques sont momentanément indisponibles.";
    if (error.code === "invalid_dashboard_response") return "Le service a renvoyé des statistiques invalides.";
    if (error.status === 401 || ["organization_not_found", "email_required", "email_not_verified"].includes(error.code)) {
      return organizationErrorMessage(error);
    }
  }
  return "Impossible de charger les statistiques. Réessayez dans un instant.";
}
