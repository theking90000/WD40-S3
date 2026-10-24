import assert from "node:assert/strict";
import test from "node:test";
import { createMemoryRouter } from "react-router";
import type { RouteObject } from "react-router";
import { QueryObserver } from "@tanstack/react-query";
import { AuthError } from "../../lib/api.ts";
import { logout } from "../auth/session.ts";
import type { CurrentUser } from "../auth/session.ts";
import { authMiddleware, requireUserMiddleware, withUser } from "../auth/router.ts";
import { organizationLoader, organizationMiddleware, selectionLoader } from "./router.ts";
import { createOrganization, deleteOrganization, organizationErrorMessage, organizationQuery, organizationsQuery, refreshOrganizations, updateOrganization } from "./api.ts";
import type { Organization } from "./api.ts";
import { currentUserKey, queryClient, resetQuerySession, setQueryUser } from "../../lib/query-client.ts";

test("organization cache and routing share requests and follow membership and session changes", async (t) => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { location: { href: "http://localhost/" }, sessionStorage: { removeItem() {} } },
  });
  t.after(() => {
    resetQuerySession();
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else Reflect.deleteProperty(globalThis, "window");
  });

  const alice: CurrentUser = {
    issuer: "https://identity.example", subject: "alice", name: "Alice",
    email: "Alice@example.org", email_verified: true, roles: [], groups: [],
  };
  const first: Organization = {
    id: "e2b219e2-f69d-41cd-83b2-e3e791133b24", name: "First",
    created_at: "2026-10-18T12:00:00+00:00", access: "OWNER",
  };
  const second: Organization = { ...first, id: "a2b219e2-f69d-41cd-83b2-e3e791133b24", name: "Second", access: "MEMBER" };
  let currentUser: CurrentUser | null = alice;
  let organizations: Organization[] = [];
  let listings = 0;
  let creations = 0;
  let childLoads = 0;
  let childActions = 0;
  let listingError: string | null = null;
  let listingWait: Promise<void> | undefined;
  let creationWait: Promise<void> | undefined;
  let nameTaken = false;
  t.mock.method(globalThis, "fetch", async (input: string | URL | Request, options: RequestInit = {}) => {
    const url = String(input);
    if (url === "/api/me") return currentUser ? Response.json(currentUser) : Response.json({ error: "unauthenticated" }, { status: 401 });
    assert.equal(options.credentials, "same-origin");
    assert.equal(options.cache, "no-store");
    if (url === "/api/logout") {
      currentUser = null;
      return new Response(null, { status: 204 });
    }
    assert.equal(url, "/api/organizations");
    if (options.method === "POST") {
      creations += 1;
      assert.equal(new Headers(options.headers).get("X-WD40-Portal"), "1");
      assert.deepEqual(JSON.parse(String(options.body)), { name: "New" });
      await creationWait;
      if (nameTaken) return Response.json({ error: "organization_name_taken" }, { status: 409 });
      return Response.json({ ...first, name: "New" }, { status: 201 });
    }
    listings += 1;
    const snapshot = [...organizations];
    await listingWait;
    return listingError ? Response.json({ error: listingError }, { status: 403 }) : Response.json(snapshot);
  });

  const routes: RouteObject[] = [{
    id: "root", path: "/", middleware: [authMiddleware], children: [
      { path: "login", id: "login" },
      {
        id: "layouts/session", middleware: [requireUserMiddleware], loader: withUser(),
        shouldRevalidate: () => true,
        children: [
          { index: true, id: "selection", loader: selectionLoader },
          {
            path: ":organizationId", id: "organization", middleware: [organizationMiddleware], loader: organizationLoader,
            children: [{
              index: true, id: "home", loader: () => { childLoads += 1; return null; },
              action: () => { childActions += 1; return null; },
            }, { path: "status", id: "status" }],
          },
        ],
      },
    ],
  }];
  const router = createMemoryRouter(routes, { initialEntries: ["/"] });
  t.after(() => router.dispose());
  async function idle() {
    if (router.state.initialized && router.state.navigation.state === "idle") return;
    await new Promise<void>((resolve) => {
      const unsubscribe = router.subscribe((state) => {
        if (state.initialized && state.navigation.state === "idle") { unsubscribe(); resolve(); }
      });
    });
  }
  await idle();
  assert.equal(router.state.errors, null);
  assert.equal(router.state.location.pathname, "/");
  assert.equal(listings, 1);

  organizations = [first];
  await refreshOrganizations(alice);
  await router.revalidate();
  await idle();
  assert.equal(router.state.location.pathname, `/${first.id}/`);
  assert.equal(listings, 2);
  assert.equal(childLoads, 1);
  await router.navigate("/?organization_deleted=1");
  assert.equal(router.state.location.pathname, "/");
  assert.equal(router.state.location.search, "?organization_deleted=1");
  await router.navigate(`/${first.id}/`);
  await router.navigate(`/${first.id}/status`);
  await router.navigate(`/${first.id}/`);
  assert.equal(listings, 2);

  organizations = [first, second];
  await refreshOrganizations(alice);
  await router.navigate("/");
  assert.equal(router.state.location.pathname, "/");
  await router.navigate(`/${second.id}/`);
  assert.equal(listings, 3);

  const blockedCount = childLoads;
  organizations = [first];
  await refreshOrganizations(alice);
  await router.navigate(`/${second.id}/`);
  assert.equal(router.state.location.pathname, "/");
  assert.equal(router.state.location.search, "?organization_unavailable=1");
  assert.equal(childLoads, blockedCount);
  assert.equal(listings, 5);
  await router.navigate(`/${second.id}/`, { formMethod: "post", formData: new FormData() });
  assert.equal(childActions, 0);

  // A first request for an inaccessible deep link needs only one fresh listing.
  resetQuerySession();
  const freshCount = listings;
  await router.navigate(`/${second.id}/`);
  assert.equal(listings, freshCount + 1);
  organizations = [first, second];
  await router.navigate(`/${second.id}/`);
  assert.equal(listings, freshCount + 2);
  assert.equal(router.state.location.pathname, `/${second.id}/`);

  const beforeCreation = listings;
  await createOrganization(alice, "  New  ");
  assert.equal(creations, 1);
  await router.navigate(`/${first.id}/`);
  assert.equal(listings, beforeCreation);
  assert.equal(queryClient.getQueryData(organizationsQuery(alice).queryKey)?.at(0)?.name, "New");
  await assert.rejects(createOrganization(alice, " "), (error) => error instanceof AuthError && error.code === "invalid_name");

  nameTaken = true;
  const cachedOrganizations = queryClient.getQueryData(organizationsQuery(alice).queryKey);
  const locationBeforeConflict = router.state.location;
  await assert.rejects(createOrganization(alice, "  New  "), (error) => {
    assert.ok(error instanceof AuthError);
    assert.equal(error.status, 409);
    assert.equal(error.code, "organization_name_taken");
    assert.equal(organizationErrorMessage(error), "Une organisation porte déjà ce nom.");
    return true;
  });
  assert.equal(queryClient.getQueryData(organizationsQuery(alice).queryKey), cachedOrganizations);
  assert.equal(router.state.location, locationBeforeConflict);
  nameTaken = false;

  resetQuerySession();
  setQueryUser(alice);
  let releaseListing: () => void = () => {};
  listingWait = new Promise<void>((resolve) => { releaseListing = resolve; });
  const beforeShared = listings;
  const one = queryClient.fetchQuery(organizationsQuery(alice));
  const two = queryClient.fetchQuery(organizationsQuery(alice));
  assert.equal(listings, beforeShared + 1);
  releaseListing();
  await Promise.all([one, two]);
  listingWait = undefined;

  // Focus/background updates notify consumers of the same cache.
  const observer = new QueryObserver(queryClient, organizationsQuery(alice));
  let observed: Organization[] | undefined;
  const unsubscribe = observer.subscribe((result) => { observed = result.data; });
  organizations = [];
  await refreshOrganizations(alice);
  assert.deepEqual(observed, []);
  unsubscribe();

  listingError = "email_not_verified";
  await assert.rejects(refreshOrganizations(alice), (error) => error instanceof AuthError && error.status === 403);
  // The backend decides whether email verification is required.
  for (const [email, email_verified, backendError] of [
    [alice.email, false, null],
    [alice.email, null, null],
    [alice.email, false, "email_not_verified"],
    [null, null, "email_required"],
    ["", true, "email_required"],
  ] as const) {
    listingError = backendError;
    currentUser = { ...alice, email, email_verified };
    await queryClient.invalidateQueries({ queryKey: currentUserKey, exact: true, refetchType: "none" });
    await queryClient.invalidateQueries({ queryKey: organizationsQuery(currentUser).queryKey, exact: true, refetchType: "none" });
    const beforeListing: number = listings;
    await router.navigate("/");
    assert.equal(listings, beforeListing + 1);
    if (backendError) {
      assert.ok(Object.values(router.state.errors ?? {}).some((error) => error instanceof AuthError
        && error.status === 403 && error.code === backendError));
      assert.equal(organizationErrorMessage(new AuthError(backendError, 403)), backendError === "email_required"
        ? "Votre compte doit fournir une adresse email pour accéder aux organisations."
        : "Une adresse email vérifiée est nécessaire pour accéder aux organisations.");
    } else {
      assert.equal(router.state.errors, null);
    }
  }
  listingError = null;

  currentUser = { ...alice, subject: "bob", email: "bob@example.org" };
  await queryClient.invalidateQueries({ queryKey: currentUserKey, exact: true, refetchType: "none" });
  await router.navigate("/");
  assert.equal(queryClient.getQueryData(organizationsQuery(alice).queryKey), undefined);
  assert.equal(router.state.loaderData["layouts/session"].user.subject, "bob");

  // Late mutations from a logged-out session cannot populate a later session.
  setQueryUser(alice);
  let releaseCreation: () => void = () => {};
  creationWait = new Promise<void>((resolve) => { releaseCreation = resolve; });
  const pendingCreation = createOrganization(alice, "New");
  await logout();
  releaseCreation();
  await assert.rejects(pendingCreation, (error) => error instanceof DOMException && error.name === "AbortError");
  assert.equal(queryClient.getQueryData(currentUserKey), null);
  assert.equal(queryClient.getQueryCache().getAll().length, 1);
  await router.navigate(`/${first.id}/`);
  assert.equal(router.state.location.pathname, "/login");
});

test("organization mutations update shared caches, cancel old reads and reject late sessions", async (t) => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { location: { href: "http://localhost/" } },
  });
  t.after(() => {
    resetQuerySession();
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else Reflect.deleteProperty(globalThis, "window");
  });
  const alice: CurrentUser = {
    issuer: "https://identity.example", subject: "alice", name: "Alice",
    email: "alice@example.org", email_verified: true, roles: [], groups: [],
  };
  const bob = { ...alice, subject: "bob", email: "bob@example.org" };
  const first: Organization = {
    id: "e2b219e2-f69d-41cd-83b2-e3e791133b24", name: "First",
    created_at: "2026-10-18T12:00:00+00:00", access: "OWNER",
  };
  const second: Organization = { ...first, id: "a2b219e2-f69d-41cd-83b2-e3e791133b24", name: "Second" };
  const renamed = { ...first, name: "Renamed" };
  const counts = { buckets: 0, credentials: 0, members: 2 };
  const listKey = organizationsQuery(alice).queryKey;
  const detailsKey = organizationQuery(alice, first.id).queryKey;
  const memberKey = [...listKey, first.id, "members"];
  const otherKey = organizationQuery(alice, second.id).queryKey;
  let errorCode: string | undefined;
  let malformed = false;
  let readWait: Promise<void> | undefined;
  let writeWait: Promise<void> | undefined;
  let listing: Organization[] = [first, second];
  let writes = 0;
  let reads = 0;

  t.mock.method(globalThis, "fetch", async (input: string | URL | Request, options: RequestInit = {}) => {
    const url = String(input);
    if (options.method === "PATCH" || options.method === "DELETE") {
      writes += 1;
      assert.equal(url, `/api/organizations/${first.id}`);
      assert.equal(new Headers(options.headers).get("X-WD40-Portal"), "1");
      assert.equal(options.credentials, "same-origin");
      if (options.method === "PATCH") assert.deepEqual(JSON.parse(String(options.body)), { name: "Renamed" });
      await writeWait;
      if (errorCode) return Response.json({ error: errorCode }, { status: errorCode === "owner_required" ? 403 : 409 });
      return options.method === "DELETE" ? new Response(null, { status: 204 })
        : Response.json(malformed ? { ...renamed, id: second.id } : renamed);
    }
    reads += 1;
    const snapshot = [...listing];
    await readWait;
    if (url === "/api/organizations") return Response.json(snapshot);
    assert.equal(url, `/api/organizations/${first.id}`);
    return Response.json({ ...first, counts });
  });

  setQueryUser(alice);
  queryClient.setQueryData(listKey, listing);
  queryClient.setQueryData(detailsKey, { ...first, counts });
  queryClient.setQueryData(memberKey, ["member@example.org"]);
  queryClient.setQueryData(otherKey, { ...second, counts });
  const observer = new QueryObserver(queryClient, organizationsQuery(alice));
  let observed: Organization[] | undefined;
  const unsubscribe = observer.subscribe((result) => { observed = result.data; });
  t.after(unsubscribe);

  let releaseReads = () => {};
  readWait = new Promise<void>((resolve) => { releaseReads = resolve; });
  const oldList = queryClient.fetchQuery({ ...organizationsQuery(alice), staleTime: 0 }).catch(() => undefined);
  const oldDetails = queryClient.fetchQuery({ ...organizationQuery(alice, first.id), staleTime: 0 }).catch(() => undefined);
  await updateOrganization(alice, first.id, " Renamed ");
  assert.equal(observed?.at(0)?.name, "Renamed");
  assert.deepEqual(queryClient.getQueryData(detailsKey), { ...renamed, counts });
  releaseReads();
  await Promise.all([oldList, oldDetails]);
  readWait = undefined;
  assert.equal(queryClient.getQueryData(organizationsQuery(alice).queryKey)?.at(0)?.name, "Renamed");
  assert.deepEqual(queryClient.getQueryData(memberKey), ["member@example.org"]);

  const beforeInvalid = writes;
  await assert.rejects(updateOrganization(alice, first.id, " "), (error) => error instanceof AuthError && error.code === "invalid_name");
  await assert.rejects(updateOrganization(alice, first.id, "é".repeat(129)), (error) => error instanceof AuthError && error.code === "invalid_name");
  assert.equal(writes, beforeInvalid);
  const cached = queryClient.getQueryData(listKey);
  errorCode = "organization_name_taken";
  await assert.rejects(updateOrganization(alice, first.id, "Renamed"), (error) => error instanceof AuthError && error.code === "organization_name_taken");
  assert.equal(queryClient.getQueryData(listKey), cached);
  errorCode = undefined;
  malformed = true;
  await assert.rejects(updateOrganization(alice, first.id, "Renamed"), (error) => error instanceof AuthError && error.code === "invalid_organization_response");
  assert.equal(queryClient.getQueryData(listKey), cached);
  malformed = false;

  errorCode = "organization_not_empty";
  await assert.rejects(deleteOrganization(alice, first.id), (error) => error instanceof AuthError && error.code === "organization_not_empty");
  assert.equal(queryClient.getQueryData(listKey), cached);
  assert.equal(queryClient.getQueryState(detailsKey)?.isInvalidated, true);
  errorCode = undefined;

  const inaccessible = { ...first, access: "MEMBER" as const };
  listing = [inaccessible, second];
  errorCode = "owner_required";
  await assert.rejects(updateOrganization(alice, first.id, "Renamed"), (error) => error instanceof AuthError && error.code === "owner_required");
  assert.equal(queryClient.getQueryData(organizationsQuery(alice).queryKey)?.at(0)?.access, "MEMBER");
  errorCode = undefined;

  listing = [renamed, second];
  queryClient.setQueryData(listKey, listing);
  readWait = new Promise<void>((resolve) => { releaseReads = resolve; });
  const oldDeletionList = queryClient.fetchQuery({ ...organizationsQuery(alice), staleTime: 0 }).catch(() => undefined);
  await deleteOrganization(alice, first.id);
  assert.deepEqual(queryClient.getQueryData(listKey), [second]);
  assert.equal(queryClient.getQueryData(detailsKey), undefined);
  assert.equal(queryClient.getQueryData(memberKey), undefined);
  assert.deepEqual(queryClient.getQueryData(otherKey), { ...second, counts });
  releaseReads();
  await oldDeletionList;
  readWait = undefined;
  assert.deepEqual(queryClient.getQueryData(listKey), [second]);
  unsubscribe();

  for (const operation of ["PATCH", "DELETE", "LATE_ERROR"] as const) {
    resetQuerySession();
    setQueryUser(alice);
    let releaseWrite = () => {};
    writeWait = new Promise<void>((resolve) => { releaseWrite = resolve; });
    errorCode = operation === "LATE_ERROR" ? "owner_required" : undefined;
    const pending = operation === "PATCH" ? updateOrganization(alice, first.id, "Renamed") : deleteOrganization(alice, first.id);
    const readsBefore = reads;
    setQueryUser(bob);
    const bobKey = organizationsQuery(bob).queryKey;
    queryClient.setQueryData(bobKey, [second]);
    releaseWrite();
    await assert.rejects(pending, (error) => error instanceof DOMException && error.name === "AbortError");
    assert.equal(queryClient.getQueryData(listKey), undefined);
    assert.deepEqual(queryClient.getQueryData(bobKey), [second]);
    assert.equal(reads, readsBefore);
  }
});
