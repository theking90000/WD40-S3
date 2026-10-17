const base = import.meta.env?.BASE_URL ?? "/";
const pendingKey = `wd40.portal.oidc:${base}`;
const callbackParams = [
  "code", "state", "iss", "session_state", "error", "error_description", "error_uri",
];
let callback;

export class AuthError extends Error {
  constructor(code, status) {
    super(code);
    this.name = "AuthError";
    this.code = code;
    this.status = status;
  }
}

async function api(path, { method = "GET", body, signal } = {}) {
  const headers = new Headers();
  if (method === "POST") headers.set("X-WD40-Portal", "1");
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
    const body = await response.json().catch(() => null);
    throw new AuthError(
      typeof body?.error === "string" ? body.error : "http_error",
      response.status,
    );
  }
  return response;
}

function consoleUrl(value) {
  const url = new URL(value, window.location.href);
  const root = new URL(base, window.location.href);
  if (url.origin !== root.origin || !url.pathname.startsWith(root.pathname)
    || url.username || url.password) {
    throw new AuthError("invalid_return_url", 400);
  }
  return url;
}

export async function getLoginUrl(returnTo = window.location.href, { signal } = {}) {
  const target = consoleUrl(returnTo);
  if (callbackParams.some((name) => target.searchParams.has(name))) {
    throw new AuthError("callback_parameters_in_return_url", 400);
  }
  const redirect = new URL(target);
  redirect.hash = "";
  const response = await api("login", {
    method: "POST", body: { redirect_uri: redirect.href }, signal,
  });
  const body = await response.json();
  if (typeof body?.url !== "string") throw new AuthError("invalid_login_response", 502);
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

export async function login(returnTo) {
  window.location.assign(await getLoginUrl(returnTo));
}

export async function completeCallback({ code, state, redirectUri }) {
  // This exchange belongs to the login attempt, not to an abortable route loader.
  await api("callback", {
    method: "POST", body: { code, state, redirect_uri: redirectUri },
  });
}

export async function getCurrentUser({ signal } = {}) {
  let response;
  try {
    response = await api("me", { signal });
  } catch (error) {
    if (error instanceof AuthError && error.status === 401) return null;
    throw error;
  }
  const user = await response.json();
  if (!user || typeof user.issuer !== "string" || !user.issuer
    || typeof user.subject !== "string" || !user.subject) {
    throw new AuthError("invalid_user_response", 502);
  }
  return user;
}

export async function logout() {
  await api("logout", { method: "POST" });
  window.sessionStorage.removeItem(pendingKey);
  callback = undefined;
}

export function finishCallback(value) {
  const url = new URL(value);
  if (!["code", "state", "error"].some((name) => url.searchParams.has(name))) {
    // A navigation can supersede the callback while its non-abortable exchange is running.
    return callback?.pending ? callback.promise.then((result) => (
      result.error ? { url: url.href, error: result.error } : null
    )) : null;
  }
  consoleUrl(url);
  if (callback?.url === url.href) return callback.promise;
  const attempt = { url: url.href, pending: true };
  attempt.promise = exchangeCallback(url).finally(() => { attempt.pending = false; });
  callback = attempt;
  return attempt.promise;
}

async function exchangeCallback(url) {
  const clean = new URL(url);
  for (const name of callbackParams) clean.searchParams.delete(name);
  let destination = clean;
  let submitted = false;
  let failure;
  try {
    const pending = JSON.parse(window.sessionStorage.getItem(pendingKey) ?? "null");
    if (typeof pending?.redirectUri !== "string" || typeof pending?.returnTo !== "string") {
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
    if (callbackParams.some((name) => params.getAll(name).length > 1)
      || !params.get("state") || params.has("code") === params.has("error")) {
      throw new AuthError("invalid_callback", 400);
    }
    if (params.has("error")) throw new AuthError("oidc_authorization_failed", 400);
    if (!params.get("code")) throw new AuthError("invalid_callback", 400);
    submitted = true;
    await completeCallback({
      code: params.get("code"),
      state: params.get("state"),
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
