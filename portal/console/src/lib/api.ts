import { querySession } from "./query-client.ts";

const base = import.meta.env?.BASE_URL ?? "/";

interface ApiOptions {
  signal?: AbortSignal;
  method?: "GET" | "POST" | "DELETE" | "PATCH";
  body?: unknown;
}

export class AuthError extends Error {
  readonly code: string;
  readonly status: number;
  readonly session: number | undefined;

  constructor(code: string, status: number, session?: number) {
    super(code);
    this.name = "AuthError";
    this.code = code;
    this.status = status;
    this.session = session;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export async function api(path: string, { method = "GET", body, signal }: ApiOptions = {}) {
  const session = querySession();
  const headers = new Headers();
  if (method === "POST" || method === "DELETE" || method === "PATCH") headers.set("X-WD40-Portal", "1");
  if (body !== undefined) headers.set("Content-Type", "application/json");
  const response = await fetch(`${base}api/${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
    cache: "no-store",
    signal,
  });
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null);
    throw new AuthError(
      isRecord(body) && typeof body.error === "string" ? body.error : "http_error",
      response.status,
      session,
    );
  }
  return response;
}
