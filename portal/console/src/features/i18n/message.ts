import type messages from "../../locales/fr/messages.ts";

type PluralSuffix = "zero" | "one" | "two" | "few" | "many" | "other";
type MessageKey = Exclude<keyof typeof messages, `${string}_${PluralSuffix}`>;

// Store identifiers and data so visible feedback follows language changes.
export interface Message {
  key: MessageKey;
  values?: Record<string, string | number>;
}
