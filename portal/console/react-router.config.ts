import type { Config } from "@react-router/dev/config";

export default {
  appDirectory: "src",
  basename: process.env.WD40_PORTAL_BASE_PATH || "/",
  ssr: false,
  prerender: ["/", "/login"],
} satisfies Config;
