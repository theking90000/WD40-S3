import type french from "../fr/common.ts";

export default {
  language: {
    label: "Language",
    automatic: "Automatic",
    automaticLanguage: "Automatic · {{language}}",
    trigger: "Language: {{language}}",
  },
} satisfies Pick<typeof french, "language">;
