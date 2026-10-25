import "i18next";
import type common from "../../locales/fr/common.ts";

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "common";
    enableSelector: true;
    resources: { common: typeof common };
  }
}
