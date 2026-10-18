import { layout, type RouteConfig } from "@react-router/dev/routes";
import { flatRoutes } from "@react-router/fs-routes";

export default [
  layout("layouts/console.tsx", await flatRoutes()),
] satisfies RouteConfig;
