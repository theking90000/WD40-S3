import { index, layout, route, type RouteConfig } from "@react-router/dev/routes";

export default [
  route("login", "routes/login.tsx"),
  layout("layouts/session.tsx", [
    index("routes/_index.tsx"),
    route(":organizationId", "layouts/console.tsx", [
      index("routes/organization.tsx"),
      route("buckets", "routes/buckets.tsx"),
      route("buckets/new", "routes/buckets-new.tsx"),
      route("credentials", "routes/credentials.tsx"),
      route("credentials/new", "routes/credentials-new.tsx"),
      route("members", "routes/members.tsx"),
      route("settings", "routes/settings.tsx"),
      route("status", "routes/status.tsx"),
    ]),
  ]),
] satisfies RouteConfig;
