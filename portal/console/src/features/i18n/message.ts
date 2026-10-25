import type messages from "../../locales/fr/messages.ts";

// Store identifiers and data so visible feedback follows language changes.
export interface Message {
  key: keyof typeof messages;
  values?: Record<string, string | number>;
}
