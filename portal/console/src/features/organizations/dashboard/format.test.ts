import frenchMessages from "../../../locales/fr/messages.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { createDashboardFormatters } from "./format.ts";
import { dashboardErrorKey } from "./api.ts";
import { AuthError } from "../../../lib/api.ts";

const { formatBytes, formatCount, formatPercentage } = createDashboardFormatters("fr-BE");

test("dashboard volumes use decimal units and preserve zero", () => {
  for (const [bytes, expected] of [[0, "0 o"], [999, "999 o"], [1000, "1 Ko"],
    [1500, "1,5 Ko"], [1_000_000, "1 Mo"], [2_450_000_000, "2,45 Go"],
    [Number.MAX_SAFE_INTEGER, "9,01 Po"]] as const) {
    assert.equal(formatBytes(bytes), expected);
  }
});

test("counts and storage shares use French formatting", () => {
  assert.equal(formatCount(1_234_567).replace(/\s/g, " "), "1 234 567");
  assert.equal(formatCount(0), "0");
  assert.equal(formatPercentage(0).replace(/\s/g, " "), "0 %");
  assert.equal(formatPercentage(0.256).replace(/\s/g, " "), "25,6 %");
  assert.equal(formatPercentage(1).replace(/\s/g, " "), "100 %");
});

test("dashboard errors distinguish unavailable metrics, invalid data and lost access", () => {
  assert.equal(frenchMessages[dashboardErrorKey(new AuthError("metrics_unavailable", 503))], "Les statistiques sont momentanément indisponibles.");
  assert.equal(frenchMessages[dashboardErrorKey(new AuthError("invalid_dashboard_response", 502))], "Le service a renvoyé des statistiques invalides.");
  assert.match(frenchMessages[dashboardErrorKey(new AuthError("unauthenticated", 401))], /session a expiré/);
  assert.match(frenchMessages[dashboardErrorKey(new AuthError("organization_not_found", 404))], /plus accès/);
  assert.match(frenchMessages[dashboardErrorKey(new TypeError("Failed to fetch"))], /Impossible de charger/);
});
