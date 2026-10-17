import assert from "node:assert/strict";
import test from "node:test";
import { createMemoryRouter } from "react-router";
import { AuthError, finishCallback, getLoginUrl, logout } from "./auth.js";
import { authMiddleware, optionalUser, withUser } from "./auth-router.js";

test("callbacks precede guarded loaders, public routes skip user lookup, and failures do not log in", async (t) => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const storage = new Map();
  globalThis.window = {
    location: { href: "http://localhost/" },
    sessionStorage: {
      getItem: (key) => storage.get(key) ?? null,
      setItem: (key, value) => storage.set(key, value),
      removeItem: (key) => storage.delete(key),
    },
  };
  t.after(() => {
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else delete globalThis.window;
  });

  const calls = [];
  let authenticated = false;
  let unavailable = false;
  const user = { issuer: "https://identity.example", subject: "alice" };
  t.mock.method(globalThis, "fetch", async (url, options) => {
    calls.push({ url, options });
    assert.equal(options.credentials, "same-origin");
    if (options.method === "POST") assert.equal(options.headers.get("X-WD40-Portal"), "1");
    if (url === "/api/login") return Response.json({ url: "https://identity.example/authorize" });
    if (url === "/api/callback") {
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
  rootLoader.hydrate = true;
  const routes = [{
    id: "root",
    path: "/",
    middleware: [authMiddleware],
    loader: rootLoader,
    children: [
      { id: "public", index: true },
      {
        id: "private", path: "private", loader: withUser(),
        children: [{
          id: "private-index", index: true,
          loader: withUser(({ user }) => {
            calls.push({ url: "business-loader" });
            return user;
          }),
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
    await new Promise((resolve) => {
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
  assert.deepEqual(calls.map(({ url }) => url), [
    "/api/login", "/api/callback", "/api/me", "business-loader",
  ]);
  assert.equal(storage.size, 0);

  unavailable = true;
  await router.revalidate();
  await idle();
  assert.ok(Object.values(router.state.errors ?? {}).some((error) => error.status === 503));
  assert.equal(calls.filter(({ url }) => url === "/api/login").length, 1);
  unavailable = false;

  await logout();
  await router.navigate("/optional");
  assert.equal(router.state.loaderData.optional, null);
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
