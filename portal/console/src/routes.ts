import { layout, route, type RouteConfig } from "@react-router/dev/routes";
import { flatRoutes } from "@react-router/fs-routes";

export default [
  route("login", "routes/login.tsx"),
  layout("layouts/console.tsx", await flatRoutes({ ignoredRouteFiles: ["routes/login.tsx"] })),
] satisfies RouteConfig;
