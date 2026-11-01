import { expect, it } from "vitest";
import { comparisonSeries } from "./comparison";
import { distanceTrace, mapBounds, tracePoint, miniSectors } from "./analysis";
import type { DriverState } from "./types/DriverState";
import type { TelemetrySample } from "./types/TelemetrySample";

const driver = (number: number, team = "A") => ({
  driver: { number, code: `D${number}`, team, color: "ff0000" }, lap: 4,
  laps: [1, 2, 3, 4].map((number) => ({ number, duration: 89 + number,
    tyre_age: number, status: number === 3 ? "SC" : "GREEN",
    pit_in: number === 4, pit_out: false })),
  stints: [{ number: 1, compound: "MEDIUM", start_lap: 1, end_lap: null }],
  prediction: { pace: [{ value: 94, low: 93, high: 95 }],
    pit_window: { from_lap: 7, to_lap: 9, probability: .6 } },
} as DriverState);

it("greys pit/SC outliers and excludes them from the rolling mean", () => {
  const series = comparisonSeries([driver(1)], "pace", false, 1, 10, []);
  expect(series.find(s => s.name === "D1")?.data).toEqual([[1, 90], [2, 91], [3, null], [4, null]]);
  expect(series.find(s => s.name === "D1 pit / SC")?.data).toEqual([[3, 92], [4, 93]]);
  expect(series.find(s => s.name === "D1 pit / SC")?.itemStyle).toMatchObject({ color: "#87909d" });
  expect(series.find(s => s.name === "D1 3-lap mean")?.data.at(-1)).toEqual([4, 91]);
});
it("styles teammates by actual team and fits each stint with forecast uncertainty", () => {
  const drivers = [driver(1), driver(2, "B"), driver(3)];
  const pace = comparisonSeries(drivers, "pace", false, 1, 10, []);
  expect(pace.find(s => s.name === "D2")?.lineStyle?.type).toBe("solid");
  expect(pace.find(s => s.name === "D3")?.lineStyle?.type).toBe("dashed");
  const deg = comparisonSeries(drivers.slice(0, 1), "degradation", false, 1, 10, []);
  expect(deg.find(s => s.name === "D1 S1 fit")?.data).toEqual([[1, 90], [2, 91]]);
  expect(deg.find(s => s.name === "D1 forecast")?.markArea?.data).toEqual([
    [{ xAxis: 4.6, yAxis: 93 }, { xAxis: 5.4, yAxis: 95 }],
  ]);
});
it("plots logged actuals with their original issued forecasts and stint pit ranges", () => {
  const records = [{ session: 0, driver: 1, issued_lap: 1, target_lap: 2,
    band: {value: 92, low: 90, high: 94}, actual: 91, model: "test" }];
  const accuracy = comparisonSeries([driver(1)], "accuracy", false, 1, 10, records);
  expect(accuracy.find(s => s.name === "D1")?.data).toEqual([[2, 91]]);
  expect(accuracy.find(s => s.name === "D1 recorded forecast")?.data).toEqual([[2, 92]]);
  const stints = comparisonSeries([driver(1)], "stints", false, 1, 10, []);
  expect(stints.find(s => s.name === "D1 predicted current stint")?.data).toEqual([[4, "D1"], [7, "D1"]]);
  expect(stints.find(s => s.name === "D1 pit window 60%")?.data).toEqual([[7, "D1"], [9, "D1"]]);
});
it("clips predicted strategy to future laps and respects the selected lap range", () => {
  const d = driver(1);
  d.prediction!.pit_window!.from_lap = 3;
  const stints = comparisonSeries([d], "stints", false, 1, 10, []);
  expect(stints.find(s => s.name === "D1 pit window 60%")?.data).toEqual([[4, "D1"], [9, "D1"]]);
  expect(comparisonSeries([d], "stints", false, 20, 30, [])).toEqual([]);
  const deg = comparisonSeries([d], "degradation", false, 1, 4, []);
  expect(deg.find(s => s.name === "D1 forecast")?.data ?? []).toEqual([]);
});
const sample = (time_ms: number, x: number | null, speed = 36) => ({
  time_ms, x, y: x == null ? null : 5000, speed, rpm: null,
  gear: null, throttle: null, brake: null, drs: null,
} satisfies TelemetrySample);
it("bounds real coordinates and aligns hover with actual positioned telemetry", () => {
  expect(mapBounds([[-5000, 5000], [-4000, 5500]])).toEqual([-5080, 4920, 1160, 660]);
  const rows = distanceTrace([sample(0, -5000), sample(1000, -4500), sample(2000, -4000)]);
  expect(tracePoint(rows, 10)).toEqual([-4500, 5000]);
  expect(tracePoint(distanceTrace([sample(0, null)]), 0)).toBeNull();
});
it("derives approximate mini-sector winners from distance-aligned lap times", () => {
  const a = distanceTrace([sample(0, -5000), sample(1000, -4500), sample(2000, -4000)]);
  const b = distanceTrace([sample(0, -5000, 18), sample(2000, -4500, 18), sample(4000, -4000, 18)]);
  expect(miniSectors(a, b, 20, 2).map(s => s.winner)).toEqual(["driver", "driver"]);
  expect(miniSectors(a, distanceTrace([sample(0, null)]), 20, 2)).toEqual([]);
});
it("does not connect missing-speed samples into positioned distance traces", () => {
  const rows = distanceTrace([sample(0, -5000), { ...sample(1000, -4500), speed: null }, sample(2000, -4000)]);
  expect(rows[1].gap).toBe(true);
  expect(rows[2].gap).toBe(true);
  expect(tracePoint(rows, -1)).toBeNull();
});
