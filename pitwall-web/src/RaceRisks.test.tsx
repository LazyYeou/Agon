import { expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import RaceRisks from "./RaceRisks";
import type { Prediction } from "./types/Prediction";
import type { Snapshot } from "./types/Snapshot";

it("shows risk horizon, target and model limitations without inventing missing probabilities", () => {
  const forecast = { probability: 0, horizon_laps: 5, model: "risk-v1", label: "Conditional on green laps", factors: ["Below baseline"] };
  const prediction = { safety_car: forecast, overtake: { target: 16, forecast: { ...forecast, probability: .25 } } } as Prediction;
  const drivers = { 16: { driver: { code: "LEC" } } } as unknown as Snapshot["drivers"];
  const html = renderToStaticMarkup(<RaceRisks prediction={prediction} drivers={drivers} />);
  expect(html).toContain("0.0%");
  expect(html).toContain("25.0%");
  expect(html).toContain("Pass LEC");
  expect(html).toContain("next 5 laps");
  expect(html).toContain("Below baseline");
  expect(html).toContain("Conditional on green laps");
  const missing = renderToStaticMarkup(<RaceRisks prediction={null} drivers={{}} />);
  expect(missing).toContain("unavailable");
  expect(missing).not.toContain("0.0%");
});
