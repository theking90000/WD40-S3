import { readFile, rm, writeFile } from "node:fs/promises";
import { relative, sep } from "node:path";
import { build } from "esbuild";

await rm("dist", { recursive: true, force: true });
const result = await build({
  entryPoints: { app: "src/app.jsx" },
  outdir: "dist/assets",
  entryNames: "[name].[hash]",
  chunkNames: "chunk.[hash]",
  assetNames: "[name].[hash]",
  bundle: true,
  treeShaking: true,
  splitting: true,
  format: "esm",
  metafile: true,
  minify: true,
  jsx: "automatic",
  loader: {
    ".png": "file",
    ".jpg": "file",
    ".jpeg": "file",
    ".svg": "file",
    ".webp": "file",
    ".woff": "file",
    ".woff2": "file",
  },
  define: { "process.env.NODE_ENV": '"production"' },
});

const entry = Object.entries(result.metafile.outputs).find(
  ([path, output]) => path.endsWith(".js") && output.entryPoint === "src/app.jsx",
);
if (!entry) throw new Error("Console build did not produce its JavaScript entry point");
const [script, output] = entry;
const url = (path) => `./${relative("dist", path).split(sep).join("/")}`;
const tags = [];
if (output.cssBundle) {
  tags.push(`<link rel="stylesheet" href="${url(output.cssBundle)}" />`);
}
tags.push(`<script type="module" src="${url(script)}"></script>`);

const template = await readFile("index.html", "utf8");
const marker = "<!-- portal:assets -->";
if (!template.includes(marker)) throw new Error(`index.html is missing ${marker}`);
await writeFile("dist/index.html", template.replace(marker, tags.join("\n    ")));
