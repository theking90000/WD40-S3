import assert from "node:assert/strict";
import test from "node:test";
import { createMemoryRouter } from "react-router";
import type { RouteObject } from "react-router";
import { AuthError, finishCallback, getLoginUrl, getReturnTo, logout } from "./auth.ts";
import type { CurrentUser } from "./auth.ts";
import { authMiddleware, loginLoader, optionalUser, requireUserMiddleware, withUser } from "./auth-router.ts";
import { currentUserKey, queryClient, resetQuerySession } from "./query-client.ts";

test("layout guards block loaders and actions, preserve login destinations, and handle callbacks before authentication", async (t) => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const storage = new Map<string, string>();
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      location: { href: "http://localhost/" },
      sessionStorage: {
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => storage.set(key, value),
        removeItem: (key: string) => storage.delete(key),
      },
    },
  });
  t.after(() => {
    resetQuerySession();
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else Reflect.deleteProperty(globalThis, "window");
  });

  const calls: Array<{ url: string; options?: RequestInit }> = [];
  let authenticated = false;
  let unavailable = false;
  const user: CurrentUser = {
    issuer: "https://identity.example", subject: "alice",
    name: null, email: null, email_verified: null, roles: [], groups: [],
  };
  t.mock.method(globalThis, "fetch", async (input: string | URL | Request, options: RequestInit = {}) => {
    const url = input instanceof Request ? input.url : String(input);
    calls.push({ url, options });
    assert.equal(options.credentials, "same-origin");
    if (options.method === "POST") assert.equal(new Headers(options.headers).get("X-WD40-Portal"), "1");
    if (url === "/api/login") return Response.json({ url: "https://identity.example/authorize" });
    if (url === "/api/callback") {
      assert.ok(typeof options.body === "string");
      const body = JSON.parse(options.body);
      if (!body.code) return Response.json({ error: "invalid_request" }, { status: 400 });
      assert.equal(body.redirect_uri, "http://localhost/private?view=a%20b");
      authenticated = true;
      return new Response(null, { status: 204 });
    }
    if (url === "/api/me") {
      if (unavailable) return Response.json({ error: "oidc_unavailable" }, { status: 503 });
      return authenticated ? Response.json(user)
        : Response.json({ error: "unauthenticated" }, { status: 401 });
    }
    if (url === "/api/logout") {
      authenticated = false;
      return new Response(null, { status: 204 });
    }
    throw new Error("Unexpected API request");
  });

  const rootLoader = () => null;
  rootLoader.hydrate = true as const;
  const routes: RouteObject[] = [{
    id: "root",
    path: "/",
    middleware: [authMiddleware],
    loader: rootLoader,
    children: [
      { id: "public", index: true },
      { id: "login", path: "login", loader: loginLoader },
      {
        id: "private", path: "private", middleware: [requireUserMiddleware], loader: withUser(),
        children: [{
          id: "private-index", index: true,
          loader: async (args) => {
            const user = await optionalUser(args);
            assert.ok(user);
            calls.push({ url: "business-loader" });
            return user;
          },
          action: () => {
            calls.push({ url: "business-action" });
            return null;
          },
        }],
      },
      { id: "optional", path: "optional", loader: optionalUser },
    ],
  }];
  const router = createMemoryRouter(routes, {
    initialEntries: ["/"], hydrationData: { loaderData: { root: null, public: null } },
  });
  t.after(() => router.dispose());

  async function idle(target = router) {
    if (target.state.initialized && target.state.navigation.state === "idle") return;
    await new Promise<void>((resolve) => {
      const unsubscribe = target.subscribe((state) => {
        if (state.initialized && state.navigation.state === "idle") {
          unsubscribe();
          resolve();
        }
      });
    });
  }
  await idle();
  assert.deepEqual(calls, []);

  for (const target of [
    "https://outside.example/private", "//outside.example/private",
    "http://alice:secret@localhost/private", "javascript:alert(1)",
    "/login?returnTo=/private", "/login/", "/LOGIN", "/%6cogin", "/private?code=unsolicited",
  ]) {
    assert.equal(getReturnTo(target), "http://localhost/");
  }
  assert.equal(getReturnTo("/private?view=a%20b#history"), "http://localhost/private?view=a%20b#history");

  await router.navigate("/private?view=a%20b#history");
  await idle();
  assert.equal(router.state.errors, null);
  assert.equal(router.state.location.pathname, "/login");
  assert.equal(new URLSearchParams(router.state.location.search).get("returnTo"), "http://localhost/private?view=a%20b#history");
  assert.deepEqual(router.state.loaderData.login, { returnTo: "http://localhost/private?view=a%20b#history" });
  assert.ok(calls.every(({ url }) => url === "/api/me"));

  await router.navigate("/private", { formMethod: "post", formData: new FormData() });
  await idle();
  assert.equal(router.state.location.pathname, "/login");
  assert.ok(calls.every(({ url }) => url === "/api/me"));

  unavailable = true;
  await queryClient.invalidateQueries({ queryKey: currentUserKey, exact: true, refetchType: "none" });
  const unavailableCount = calls.length;
  await router.navigate("/private");
  await idle();
  assert.ok(Object.values(router.state.errors ?? {}).some((error) => (
    error instanceof AuthError && error.status === 503
  )));
  assert.deepEqual(calls.slice(unavailableCount).map(({ url }) => url), ["/api/me"]);
  unavailable = false;

  const callbackCount = calls.length;
  await getLoginUrl("http://localhost/private?view=a%20b#history");
  const callback = "http://localhost/private?view=a%20b&code=one&state=expected";
  const first = finishCallback(callback);
  assert.equal(finishCallback(callback), first);
  await router.navigate("/private?view=a%20b&code=one&state=expected");
  await idle();
  assert.equal(router.state.errors, null);
  assert.equal(router.state.location.search, "?view=a%20b");
  assert.equal(router.state.location.hash, "#history");
  assert.deepEqual(router.state.loaderData.private, { user });
  assert.deepEqual(calls.slice(callbackCount).map(({ url }) => url), [
    "/api/login", "/api/callback", "/api/me", "business-loader",
  ]);
  assert.equal(storage.size, 0);

  const cachedUserCalls = calls.filter(({ url }) => url === "/api/me").length;
  await router.navigate(`/login?returnTo=${encodeURIComponent("/private?view=a%20b#history")}`);
  await idle();
  assert.equal(router.state.errors, null);
  assert.equal(router.state.location.pathname, "/private");
  assert.equal(router.state.location.search, "?view=a%20b");
  assert.equal(router.state.location.hash, "#history");
  assert.equal(calls.filter(({ url }) => url === "/api/me").length, cachedUserCalls);

  unavailable = true;
  await queryClient.invalidateQueries({ queryKey: currentUserKey, exact: true, refetchType: "none" });
  await router.revalidate();
  await idle();
  assert.ok(Object.values(router.state.errors ?? {}).some((error) => (
    error instanceof AuthError && error.status === 503
  )));
  assert.equal(calls.filter(({ url }) => url === "/api/login").length, 1);
  unavailable = false;

  await logout();
  const afterLogout = calls.length;
  await router.navigate("/optional");
  assert.equal(router.state.loaderData.optional, null);
  const logoutCount = calls.length;
  await router.navigate("/private");
  await idle();
  assert.equal(router.state.location.pathname, "/login");
  assert.ok(calls.slice(logoutCount).every(({ url }) => url === "/api/me"));
  assert.equal(calls.length, afterLogout);
  await router.navigate("/");
  const count = calls.length;
  await router.navigate("/?code=unsolicited&state=missing-context");
  await idle();
  assert.equal(router.state.location.search, "");
  assert.ok(Object.values(router.state.errors ?? {}).some((error) => error instanceof AuthError));
  assert.deepEqual(calls.slice(count).map(({ url }) => url), ["/api/callback"]);

  const initialCount = calls.length;
  await getLoginUrl("http://localhost/private?view=a%20b#history");
  const initialRouter = createMemoryRouter(routes, {
    initialEntries: ["/private?view=a%20b&code=two&state=expected"],
    hydrationData: { loaderData: { root: null } },
  });
  t.after(() => initialRouter.dispose());
  await idle(initialRouter);
  assert.equal(initialRouter.state.errors, null);
  assert.deepEqual(initialRouter.state.loaderData.private, { user });
  assert.deepEqual(calls.slice(initialCount).map(({ url }) => url), [
    "/api/login", "/api/callback", "/api/me", "business-loader",
  ]);
});
