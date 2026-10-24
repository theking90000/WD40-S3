import assert from "node:assert/strict";
import test from "node:test";
import { isCancelledError } from "@tanstack/react-query";
import type { CurrentUser } from "../../auth/session.ts";
import { deleteOrganization, organizationsQuery } from "../api.ts";
import { AuthError } from "../../../lib/api.ts";
import { currentUserKey, queryClient, querySession, resetQuerySession, setQueryUser } from "../../../lib/query-client.ts";
import { organizationDashboardQuery } from "./api.ts";
import type { OrganizationDashboard } from "./api.ts";

const alice: CurrentUser = {
  issuer: "https://identity.example", subject: "alice", name: "Alice",
  email: "alice@example.org", email_verified: true, roles: [], groups: [],
};
const bob: CurrentUser = { ...alice, subject: "bob", email: "bob@example.org" };
const organizationId = "e2b219e2-f69d-41cd-83b2-e3e791133b24";
const otherId = "a2b219e2-f69d-41cd-83b2-e3e791133b24";
const dashboard: OrganizationDashboard = {
  generated_at: 1_792_844_400,
  period: { from: 1_792_756_800, to: 1_792_844_400, resolution_seconds: 1800 },
  stored_bytes: 1500, stored_objects: 3, requests: 80,
  uploaded_bytes: 2000, downloaded_bytes: 4000, transferred_bytes: 6000,
};

function deferred() {
  let resolve = () => {};
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

function read() {
  return queryClient.fetchQuery({ ...organizationDashboardQuery(alice, organizationId), staleTime: 0, retry: false });
}

function invalidResponse(error: unknown) {
  return error instanceof AuthError && error.code === "invalid_dashboard_response" && error.status === 502;
}

test.beforeEach(() => {
  resetQuerySession();
  setQueryUser(alice);
});
test.afterEach(() => resetQuerySession());

test("dashboard requests share the organization cache and preserve the server totals", async (t) => {
  const wait = deferred();
  let requests = 0;
  t.mock.method(globalThis, "fetch", async (input: string | URL | Request, options: RequestInit) => {
    requests += 1;
    assert.equal(String(input), `/api/organizations/${organizationId}/dashboard`);
    assert.equal(options.method, "GET");
    assert.equal(options.credentials, "same-origin");
    assert.equal(options.cache, "no-store");
    assert.ok(options.signal instanceof AbortSignal);
    await wait.promise;
    return Response.json(dashboard);
  });
  const options = organizationDashboardQuery(alice, organizationId);
  assert.deepEqual(options.queryKey, [...organizationsQuery(alice).queryKey, organizationId, "dashboard"]);
  assert.notDeepEqual(options.queryKey, organizationDashboardQuery(alice, otherId).queryKey);
  assert.notDeepEqual(options.queryKey, organizationDashboardQuery(bob, organizationId).queryKey);
  assert.equal(options.staleTime, 30_000);
  assert.equal(options.refetchInterval, 60_000);
  assert.equal(options.refetchIntervalInBackground, false);
  const first = queryClient.fetchQuery(options);
  const second = queryClient.fetchQuery(options);
  wait.resolve();
  assert.deepEqual(await first, dashboard);
  assert.deepEqual(await second, dashboard);
  assert.deepEqual(await queryClient.fetchQuery(options), dashboard);
  assert.equal(requests, 1);
  assert.equal(queryClient.getQueryData(organizationDashboardQuery(alice, otherId).queryKey), undefined);
});

test("dashboard response validation rejects invalid metrics, periods and JSON", async (t) => {
  let body: unknown;
  let malformedJson = false;
  t.mock.method(globalThis, "fetch", async () => malformedJson ? new Response("{") : Response.json(body));
  const invalid: unknown[] = [
    null, {}, { ...dashboard, period: null },
    { ...dashboard, generated_at: "2026-10-24" },
    { ...dashboard, generated_at: Number.MAX_SAFE_INTEGER },
    { ...dashboard, period: { ...dashboard.period, from: -1 } },
    { ...dashboard, period: { ...dashboard.period, from: dashboard.period.to } },
    { ...dashboard, period: { ...dashboard.period, to: dashboard.generated_at + 1 } },
    { ...dashboard, period: { ...dashboard.period, resolution_seconds: 0 } },
    { ...dashboard, period: { ...dashboard.period, resolution_seconds: 1.5 } },
  ];
  for (const field of ["stored_bytes", "stored_objects", "requests", "uploaded_bytes", "downloaded_bytes", "transferred_bytes"]) {
    for (const value of [undefined, -1, 0.5, "1", Number.MAX_SAFE_INTEGER + 1]) {
      invalid.push({ ...dashboard, [field]: value });
    }
  }
  for (body of invalid) {
    await assert.rejects(read(), invalidResponse);
    assert.equal(queryClient.getQueryData(organizationDashboardQuery(alice, organizationId).queryKey), undefined);
  }
  malformedJson = true;
  await assert.rejects(read(), invalidResponse);
  malformedJson = false;
  body = { ...dashboard, stored_bytes: 0, stored_objects: 0, requests: 0,
    uploaded_bytes: 0, downloaded_bytes: 0, transferred_bytes: 0 };
  assert.deepEqual(await read(), body);
});

test("metrics outages preserve cached values and the authenticated session", async (t) => {
  t.mock.method(globalThis, "fetch", async () => Response.json({ error: "metrics_unavailable" }, { status: 503 }));
  const options = organizationDashboardQuery(alice, organizationId);
  const session = querySession();
  queryClient.setQueryData(currentUserKey, alice);
  queryClient.setQueryData(options.queryKey, dashboard);
  await assert.rejects(read(), (error) => error instanceof AuthError && error.code === "metrics_unavailable" && error.session === session);
  assert.equal(querySession(), session);
  assert.deepEqual(queryClient.getQueryData(options.queryKey), dashboard);
  assert.deepEqual(queryClient.getQueryData(currentUserKey), alice);
  if (typeof options.retry !== "function") throw new Error("Missing retry policy");
  assert.equal(options.retry(0, new AuthError("metrics_unavailable", 503)), true);
  assert.equal(options.retry(1, new AuthError("metrics_unavailable", 503)), false);
  assert.equal(options.retry(0, new AuthError("organization_not_found", 404)), false);
  assert.equal(options.retry(0, new DOMException("Aborted", "AbortError")), false);
});

test("an unauthorized dashboard response expires the current session through the shared cache", async (t) => {
  t.mock.method(globalThis, "fetch", async () => Response.json({ error: "unauthenticated" }, { status: 401 }));
  const options = organizationDashboardQuery(alice, organizationId);
  queryClient.setQueryData(currentUserKey, alice);
  queryClient.setQueryData(options.queryKey, dashboard);
  await assert.rejects(read(), (error) => error instanceof AuthError && error.status === 401);
  assert.equal(queryClient.getQueryData(currentUserKey), null);
  assert.equal(queryClient.getQueryData(options.queryKey), undefined);
});

test("lost membership refreshes organizations before propagating the dashboard error", async (t) => {
  const requests: string[] = [];
  t.mock.method(globalThis, "fetch", async (input: string | URL | Request) => {
    requests.push(String(input));
    if (String(input) === "/api/organizations") return Response.json([]);
    return Response.json({ error: "organization_not_found" }, { status: 404 });
  });
  queryClient.setQueryData(organizationsQuery(alice).queryKey, [{
    id: organizationId, name: "Photos", created_at: "2026-10-24T12:00:00Z", access: "MEMBER",
  }]);
  await assert.rejects(read(), (error) => error instanceof AuthError && error.code === "organization_not_found");
  assert.deepEqual(requests, [`/api/organizations/${organizationId}/dashboard`, "/api/organizations"]);
  assert.deepEqual(queryClient.getQueryData(organizationsQuery(alice).queryKey), []);
});

test("late dashboard responses and errors cannot affect a new session", async (t) => {
  for (const status of [200, 401, 404]) {
    await t.test(`late HTTP ${status}`, async (t) => {
      resetQuerySession();
      setQueryUser(alice);
      const started = deferred();
      const wait = deferred();
      let requests = 0;
      t.mock.method(globalThis, "fetch", async () => {
        requests += 1;
        started.resolve();
        await wait.promise;
        return status === 200 ? Response.json(dashboard) : Response.json({
          error: status === 401 ? "unauthenticated" : "organization_not_found",
        }, { status });
      });
      const options = organizationDashboardQuery(alice, organizationId);
      if (typeof options.queryFn !== "function") throw new Error("Missing dashboard query function");
      const pending = options.queryFn({ queryKey: options.queryKey, client: queryClient,
        signal: new AbortController().signal, meta: undefined });
      await started.promise;
      setQueryUser(bob);
      queryClient.setQueryData(currentUserKey, bob);
      const bobKey = organizationDashboardQuery(bob, organizationId).queryKey;
      queryClient.setQueryData(bobKey, dashboard);
      const rejected = assert.rejects(Promise.resolve(pending), (error) => error instanceof DOMException && error.name === "AbortError");
      wait.resolve();
      await rejected;
      assert.equal(requests, 1);
      assert.deepEqual(queryClient.getQueryData(currentUserKey), bob);
      assert.deepEqual(queryClient.getQueryData(bobKey), dashboard);
      assert.equal(queryClient.getQueryData(options.queryKey), undefined);
    });
  }
});

test("a session change during JSON parsing rejects the old body", async (t) => {
  const started = deferred();
  const wait = deferred();
  t.mock.method(globalThis, "fetch", async () => {
    const response = Response.json(dashboard);
    response.json = async () => { started.resolve(); await wait.promise; return dashboard; };
    return response;
  });
  const options = organizationDashboardQuery(alice, organizationId);
  if (typeof options.queryFn !== "function") throw new Error("Missing dashboard query function");
  const pending = options.queryFn({ queryKey: options.queryKey, client: queryClient,
    signal: new AbortController().signal, meta: undefined });
  await started.promise;
  resetQuerySession();
  setQueryUser(alice);
  const rejected = assert.rejects(Promise.resolve(pending), (error) => error instanceof DOMException && error.name === "AbortError");
  wait.resolve();
  await rejected;
});

test("session changes cancel dashboard reads and organization deletion removes their cache", async (t) => {
  const started = deferred();
  const wait = deferred();
  let signal: AbortSignal | null | undefined;
  t.mock.method(globalThis, "fetch", async (_input: string | URL | Request, options: RequestInit) => {
    if (options.method === "DELETE") return new Response(null, { status: 204 });
    signal = options.signal;
    started.resolve();
    await wait.promise;
    return Response.json(dashboard);
  });
  const options = organizationDashboardQuery(alice, organizationId);
  const pending = queryClient.fetchQuery(options);
  const rejected = assert.rejects(pending, isCancelledError);
  await started.promise;
  setQueryUser(null);
  assert.equal(signal?.aborted, true);
  wait.resolve();
  await rejected;
  assert.equal(queryClient.getQueryData(options.queryKey), undefined);

  setQueryUser(alice);
  const otherKey = organizationDashboardQuery(alice, otherId).queryKey;
  queryClient.setQueryData(options.queryKey, dashboard);
  queryClient.setQueryData(otherKey, dashboard);
  await deleteOrganization(alice, organizationId);
  assert.equal(queryClient.getQueryData(options.queryKey), undefined);
  assert.deepEqual(queryClient.getQueryData(otherKey), dashboard);
});
