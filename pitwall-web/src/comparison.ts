import type { DriverState } from "./types/DriverState";
import type { PaceForecast } from "./types/PaceForecast";
import type { ChartKind } from "./view";
import { tyreColor } from "./view";
import { chartRows, rolling } from "./analysis";
type Series = {
  [key: string]: unknown;
  id: string; name: string; type: string; data: unknown[][];
  lineStyle?: { color: string; type?: string; width?: number; opacity?: number };
  markArea?: { silent: boolean; itemStyle: { color: string; opacity: number }; data: { xAxis: number; yAxis: number | string }[][] };
};
export const teammateDash = (drivers: DriverState[], d: DriverState) =>
  drivers.filter(other => other.driver.team === d.driver.team)
    .sort((a, b) => a.driver.number - b.driver.number)[0]?.driver.number !== d.driver.number;
export const cleanLap = (lap: DriverState["laps"][number]) =>
  !lap.pit_in && !lap.pit_out && !["SC", "VSC", "RED"].includes(lap.status);

export function comparisonSeries(drivers: DriverState[], kind: ChartKind, fuel: boolean, from: number, to: number, forecasts: PaceForecast[]): Series[] {
  const series: Series[] = [];
  for (const d of drivers) {
    const color = `#${d.driver.color}`;
    const id = `${d.driver.number}-${kind}`;
    const line = (name: string, data: unknown[][], style = {}, suffix = name) => {
      const result: Series = { id: `${id}-${suffix}`, name, type: "line", data,
        showSymbol: false, connectNulls: false,
        lineStyle: { color, width: 2, type: teammateDash(drivers, d) ? "dashed" : "solid", ...style }, itemStyle: { color } };
      series.push(result);
      return result;
    };
    const band = (entry: Series, values: { x: number; low: number; high: number }[]) => {
      entry.markArea = { silent: true, itemStyle: { color, opacity: .12 },
        data: values.map(v => [{ xAxis: v.x - .4, yAxis: v.low }, { xAxis: v.x + .4, yAxis: v.high }]) };
    };
    const laps = d.laps.filter(l => l.number >= from && l.number <= to);
    if (kind === "stints") {
      for (const stint of d.stints) {
        const start = Math.max(from, stint.start_lap), end = Math.min(to, stint.end_lap ?? d.lap);
        if (start <= end) line(`${d.driver.code} S${stint.number} ${stint.compound}`, [[start, d.driver.code], [end + 1, d.driver.code]], { color: tyreColor(stint.compound), width: 18 });
      }
      const pit = d.prediction?.pit_window;
      if (pit && pit.to_lap > d.lap && pit.from_lap <= to && pit.to_lap >= from) {
        const start = Math.max(from, d.lap), end = Math.max(start, Math.min(to, pit.from_lap));
        line(`${d.driver.code} predicted current stint`, [[start, d.driver.code], [end, d.driver.code]], { type: "dashed", width: 12, opacity: .5 });
        line(`${d.driver.code} pit window ${Math.round(pit.probability * 100)}%`, [[Math.max(from, d.lap, pit.from_lap), d.driver.code], [Math.min(to, pit.to_lap), d.driver.code]], { width: 22, opacity: .2 });
      }
      continue;
    }
    if (kind === "degradation") {
      for (const stint of d.stints) {
        const values = laps.filter(l => l.number >= stint.start_lap && l.number <= (stint.end_lap ?? d.lap) && cleanLap(l) && l.duration != null && l.tyre_age != null)
          .map(l => [l.tyre_age!, l.duration! + (fuel ? l.number * .035 : 0)]);
        line(`${d.driver.code} S${stint.number}`, values, {}, `stint-${stint.number}`);
        if (values.length >= 2) {
          const mx = values.reduce((s, v) => s + v[0], 0) / values.length;
          const my = values.reduce((s, v) => s + v[1], 0) / values.length;
          const variance = values.reduce((s, v) => s + (v[0] - mx) ** 2, 0);
          if (variance) {
            const slope = values.reduce((s, v) => s + (v[0] - mx) * (v[1] - my), 0) / variance;
            const ages = [Math.min(...values.map(v => v[0])), Math.max(...values.map(v => v[0]))];
            line(`${d.driver.code} S${stint.number} fit`, ages.map(age => [age, my + slope * (age - mx)]), { width: 1, opacity: .6 });
          }
        }
      }
      const age = d.laps.at(-1)?.tyre_age;
      if (age != null && d.prediction?.pace.length) {
        const values = d.prediction.pace.flatMap((b, h) => d.lap + h + 1 < from || d.lap + h + 1 > to ? [] : [{ x: age + h + 1, value: b.value + (fuel ? (d.lap + h + 1) * .035 : 0), low: b.low + (fuel ? (d.lap + h + 1) * .035 : 0), high: b.high + (fuel ? (d.lap + h + 1) * .035 : 0) }]);
        band(line(`${d.driver.code} forecast`, values.map(v => [v.x, v.value]), { type: "dashed" }), values);
      }
      continue;
    }
    if (kind === "accuracy") {
      const records = forecasts.filter(p => p.driver === d.driver.number && p.target_lap === p.issued_lap + 1 && p.target_lap >= from && p.target_lap <= to).sort((a, b) => a.target_lap - b.target_lap);
      line(d.driver.code, records.map(p => [p.target_lap, p.actual]));
      band(line(`${d.driver.code} recorded forecast`, records.map(p => [p.target_lap, p.band.value]), { type: "dashed" }), records.map(p => ({ x: p.target_lap, ...p.band })));
      continue;
    }
    let data = chartRows(d, kind, fuel, from, to);
    if ((kind === "gap" || kind === "sectors") && drivers[0]) {
      const reference = new Map(chartRows(drivers[0], kind === "gap" ? "leader" : "sectors", false, from, to));
      data = data.map(([lap, value]) => [lap, value == null || reference.get(lap) == null ? null : value - reference.get(lap)!]);
    }
    if (kind === "pace") {
      const clean = new Set(laps.filter(cleanLap).map(l => l.number));
      const outliers = data.filter(([lap]) => !clean.has(lap));
      data = data.map(([lap, value]) => [lap, clean.has(lap) ? value : null]);
      const grey = line(`${d.driver.code} pit / SC`, outliers, { color: "#87909d" });
      grey.type = "scatter";
      grey.itemStyle = { color: "#87909d" };
    }
    const actual = line(d.driver.code, data);
    if (kind === "sectors") actual.type = "bar";
    if (kind === "position") actual.showSymbol = true;
    if (kind === "pace") {
      const mean = rolling(data.map(([, value]) => value), 3);
      line(`${d.driver.code} 3-lap mean`, data.map(([lap], i) => [lap, mean[i]]), { width: 1, opacity: .5, type: "dotted" });
    }
  }
  return series;
}
