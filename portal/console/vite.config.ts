import { fileURLToPath } from "node:url";
import { reactRouter } from "@react-router/dev/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig, loadEnv } from "vite";
import { parseS3Endpoint } from "./src/lib/s3.ts";

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "VITE_");
  if (env.VITE_S3_ENDPOINT?.trim()) parseS3Endpoint(env.VITE_S3_ENDPOINT);

  return {
    base: process.env.WD40_PORTAL_BASE_PATH || "/",
    plugins: [tailwindcss(), reactRouter()],
    resolve: {
      alias: {
        "~": fileURLToPath(new URL("./src", import.meta.url)),
      },
    },
  };
});
