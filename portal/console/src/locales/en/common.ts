import type french from "../fr/common.ts";
import messages from "./messages.ts";
import routeTitles from "./route-titles.ts";

export default {
  messages,
  routeTitles,
  language: {
    label: "Language",
    automatic: "Automatic",
    automaticLanguage: "Automatic · {{language}}",
    trigger: "Language: {{language}}",
  },
} satisfies typeof french;
