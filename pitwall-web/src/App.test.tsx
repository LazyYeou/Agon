import { expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { Snapshot } from "./types/Snapshot";
const timing = vi.hoisted(() => ({ state: null as Snapshot | null, status: "connected" }));
vi.mock("./stream", () => ({ useTiming: () => ({ ...timing, error: "", send() {}, setError() {}, samples: null, evaluation: null }) }));
import App from "./App";
vi.stubGlobal("location", {search: ""});
const state = (): Snapshot => ({
  session: { key: 1, year: 2025, event: "Test", name: "Race", circuit: "Test", country: "Test", start_ms: 0, end_ms: 1000, total_laps: null, synthetic: false },
  clock_ms: 500, playing: true, speed: 1, lap: 1, track_status: "GREEN", data_status: "live",
  weather: { track: null, air: null, humidity: null, wind_speed: null, wind_direction: null, rain: null },
  drivers: {}, messages: [], circuit: { outline: [], sectors: [], drs_zones: [], pit_lane: [], corners: [], source: "Test", pit_loss: null, approximate: true },
  qualifying_phase: null, qualifying_start_ms: null, revision: 1,
} as Snapshot);
it("labels live versus delayed source data and disables replay commands in both states", () => {
  timing.state = state();
  let html = renderToStaticMarkup(<App />);
  expect(html).toContain("OpenF1 live");
  expect(html).toMatch(/aria-label="Pause replay"[^>]*disabled=""/);
  expect(html).toMatch(/aria-label="Replay speed"[^>]*disabled=""/);
  timing.state.data_status = "data delayed";
  html = renderToStaticMarkup(<App />);
  expect(html).toContain("Live data delayed");
  expect(html).toMatch(/aria-label="Replay timeline"[^>]*disabled=""/);
});
it("distinguishes trained forecasts from basic estimates without hiding failed targets", () => {
  timing.state = state();
  timing.state.drivers[1] = { driver: { number: 1, name: "Test Driver", code: "AAA", team: "Test", color: "ff0000" },
    position: 1, start_position: 1, gap: 0, interval: 0, lap: 1, laps: [], stints: [], pits: [], in_pit: false,
    telemetry: null, location: null, partial_lap: null, tyre_age: null, prediction: { driver: 1, lap: 1, model: "race-v2", label: "model estimate · backtest below target", status: "model estimate", confidence: .8, pace: [], degradation: null, laps_to_cliff: null, tyre_health: null, pit_window: null, finish: [], rejoin: null, qualifying_need: null, factors: [], insight: "Test" } };
  expect(renderToStaticMarkup(<App />)).toContain("Trained model · below target");
  for (const model of ["basic-v1", "basic-bayesian-v2"]) {
    timing.state.drivers[1]!.prediction!.model = model;
    expect(renderToStaticMarkup(<App />)).toContain("Basic estimates");
  }
});
