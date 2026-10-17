import { reactRouter } from "@react-router/dev/vite";
import { defineConfig } from "vite";

export default defineConfig({
  base: process.env.WD40_PORTAL_BASE_PATH || "/",
  plugins: [reactRouter()],
});
