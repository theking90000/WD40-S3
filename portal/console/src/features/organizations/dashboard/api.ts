import { queryOptions } from "@tanstack/react-query";
import { api, AuthError } from "../../../lib/api.ts";
import { querySession } from "../../../lib/query-client.ts";
import type { CurrentUser } from "../../auth/session.ts";
import { organizationsQuery, refreshOrganizations } from "../api.ts";

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

function requireSession(session: number) {
  if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
}

export function organizationDashboardQuery(user: CurrentUser, organizationId: string) {
  return queryOptions({
    queryKey: [...organizationsQuery(user).queryKey, organizationId, "dashboard"],
    queryFn: async ({ signal }): Promise<OrganizationDashboard> => {
      const session = querySession();
      try {
        const response = await api(`organizations/${encodeURIComponent(organizationId)}/dashboard`, { signal });
        requireSession(session);
        const body: unknown = await response.json().catch((error: unknown) => {
          if (error instanceof SyntaxError) return null;
          throw error;
        });
        requireSession(session);
        if (!isDashboard(body)) throw new AuthError("invalid_dashboard_response", 502, session);
        return body;
      } catch (error) {
        requireSession(session);
        if (error instanceof AuthError && error.code === "organization_not_found") {
          await refreshOrganizations(user).catch(() => {});
        }
        requireSession(session);
        throw error;
      }
    },
    staleTime: 30_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
    retry: (count, error) => count < 1 && error.name !== "AbortError"
      && !(error instanceof AuthError && error.status < 500),
  });
}
