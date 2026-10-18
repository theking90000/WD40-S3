import { index, layout, route, type RouteConfig } from "@react-router/dev/routes";

export default [
  route("login", "routes/login.tsx"),
  layout("layouts/session.tsx", [
    index("routes/_index.tsx"),
    route(":organizationId", "layouts/console.tsx", [
      index("routes/organization.tsx"),
      route("status", "routes/status.tsx"),
    ]),
  ]),
] satisfies RouteConfig;
