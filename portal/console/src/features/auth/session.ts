import { api, AuthError } from "../../lib/api.ts";
import { expireQuerySession, querySession, resetQuerySession } from "../../lib/query-client.ts";

const base = import.meta.env?.BASE_URL ?? "/";
const pendingKey = `wd40.portal.oidc:${base}`;
const callbackParams = [
  "code", "state", "iss", "session_state", "error", "error_description", "error_uri",
];
export interface CurrentUser {
  issuer: string;
  subject: string;
  name: string | null;
  email: string | null;
  email_verified: boolean | null;
  roles: string[];
  groups: string[];
}

interface RequestOptions {
  signal?: AbortSignal;
}

interface CallbackInput {
  code?: string;
  state?: string;
  redirectUri?: string;
}

interface CallbackResult {
  url: string;
  error: unknown;
}

interface CallbackAttempt {
  url: string;
  pending: boolean;
  promise: Promise<CallbackResult>;
}

let callback: CallbackAttempt | undefined;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function consoleUrl(value: string | URL) {
  const url = new URL(value, window.location.href);
  const root = new URL(base, window.location.href);
  if (url.origin !== root.origin || !url.pathname.startsWith(root.pathname)
    || url.username || url.password) {
    throw new AuthError("invalid_return_url", 400);
  }
  return url;
}

export function getReturnTo(value?: string | null) {
  const root = new URL(base, window.location.href);
  try {
    const target = consoleUrl(value ?? root.href);
    const loginPath = new URL("login", root).pathname;
    if (decodeURIComponent(target.pathname).replace(/\/+$/, "").toLowerCase() === loginPath.toLowerCase()
      || callbackParams.some((name) => target.searchParams.has(name))) {
      return root.href;
    }
    return target.href;
  } catch {
    return root.href;
  }
}

export async function getLoginUrl(returnTo = window.location.href, { signal }: RequestOptions = {}) {
  const target = consoleUrl(returnTo);
  if (callbackParams.some((name) => target.searchParams.has(name))) {
    throw new AuthError("callback_parameters_in_return_url", 400);
  }
  const redirect = new URL(target);
  redirect.hash = "";
  const response = await api("login", {
    method: "POST", body: { redirect_uri: redirect.href }, signal,
  });
  const body: unknown = await response.json();
  if (!isRecord(body) || typeof body.url !== "string") {
    throw new AuthError("invalid_login_response", 502);
  }
  const authorization = new URL(body.url);
  if (authorization.protocol !== "https:" && authorization.protocol !== "http:") {
    throw new AuthError("invalid_login_response", 502);
  }
  window.sessionStorage.setItem(pendingKey, JSON.stringify({
    redirectUri: redirect.href,
    returnTo: target.href,
  }));
  return authorization.href;
}

export async function login(returnTo?: string) {
  window.location.assign(await getLoginUrl(returnTo));
}

export async function completeCallback({ code, state, redirectUri }: CallbackInput) {
  const session = querySession();
  // This exchange belongs to the login attempt, not to an abortable route loader.
  await api("callback", {
    method: "POST", body: { code, state, redirect_uri: redirectUri },
  });
  if (session !== querySession()) throw new DOMException("Session changed", "AbortError");
  resetQuerySession();
}

export async function getCurrentUser({ signal }: RequestOptions = {}): Promise<CurrentUser | null> {
  let response;
  try {
    response = await api("me", { signal });
  } catch (error) {
    if (error instanceof AuthError && error.status === 401) return null;
    throw error;
  }
  const user: CurrentUser | null = await response.json();
  if (!user || typeof user.issuer !== "string" || !user.issuer
    || typeof user.subject !== "string" || !user.subject) {
    throw new AuthError("invalid_user_response", 502);
  }
  return user;
}

export async function logout() {
  const session = querySession();
  await api("logout", { method: "POST" });
  if (session === querySession()) expireQuerySession();
  window.sessionStorage.removeItem(pendingKey);
  callback = undefined;
}

export function finishCallback(value: string | URL): Promise<CallbackResult | null> | null {
  const url = new URL(value);
  if (!["code", "state", "error"].some((name) => url.searchParams.has(name))) {
    // A navigation can supersede the callback while its non-abortable exchange is running.
    return callback?.pending ? callback.promise.then((result) => (
      result.error ? { url: url.href, error: result.error } : null
    )) : null;
  }
  consoleUrl(url);
  if (callback?.url === url.href) return callback.promise;
  const attempt: CallbackAttempt = {
    url: url.href,
    pending: true,
    promise: exchangeCallback(url),
  };
  attempt.promise = attempt.promise.finally(() => { attempt.pending = false; });
  callback = attempt;
  return attempt.promise;
}

async function exchangeCallback(url: URL): Promise<CallbackResult> {
  const clean = new URL(url);
  for (const name of callbackParams) clean.searchParams.delete(name);
  let destination = clean;
  let submitted = false;
  let failure: unknown;
  try {
    const pending: unknown = JSON.parse(window.sessionStorage.getItem(pendingKey) ?? "null");
    if (!isRecord(pending) || typeof pending.redirectUri !== "string"
      || typeof pending.returnTo !== "string") {
      throw new AuthError("missing_login_context", 400);
    }
    const redirect = consoleUrl(pending.redirectUri);
    const returnTo = consoleUrl(pending.returnTo);
    const withoutHash = new URL(returnTo);
    withoutHash.hash = "";
    if (redirect.href !== withoutHash.href || redirect.origin !== clean.origin
      || redirect.pathname !== clean.pathname
      || redirect.searchParams.toString() !== clean.searchParams.toString()) {
      throw new AuthError("invalid_login_context", 400);
    }
    destination = returnTo;
    const params = url.searchParams;
    const code = params.get("code");
    const state = params.get("state");
    if (callbackParams.some((name) => params.getAll(name).length > 1)
      || !state || params.has("code") === params.has("error")) {
      throw new AuthError("invalid_callback", 400);
    }
    if (params.has("error")) throw new AuthError("oidc_authorization_failed", 400);
    if (!code) throw new AuthError("invalid_callback", 400);
    submitted = true;
    await completeCallback({
      code,
      state,
      redirectUri: pending.redirectUri,
    });
  } catch (error) {
    failure = error;
    if (!submitted) {
      // Rust expires pending cookies even for malformed input. Preserve the original error.
      await completeCallback({}).catch(() => {});
    }
  } finally {
    try {
      window.sessionStorage.removeItem(pendingKey);
    } catch (error) {
      failure ??= error;
    }
  }
  return { url: destination.href, error: failure };
}
