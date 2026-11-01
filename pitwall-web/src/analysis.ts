import type { TelemetrySample } from "./types/TelemetrySample";
import type { DriverState } from "./types/DriverState";
import type { ChartKind } from "./view";
import type { PaceForecast } from "./types/PaceForecast";
export function mergeForecasts(previous: PaceForecast[], incoming: PaceForecast[]) {
  const records = new Map<string, PaceForecast>();
  for (const record of [...previous, ...incoming]) {
    records.set(`${record.session}-${record.driver}-${record.issued_lap}-${record.target_lap}-${record.model}`, record);
  }
  return [...records.values()];
}
export function distanceTrace(samples: TelemetrySample[]) {
  let distance = 0;
  return [...samples]
    .sort((a, b) => a.time_ms - b.time_ms)
    .map((sample, i, list) => {
      const previous = list[i - 1];
      const dt = previous ? (sample.time_ms - previous.time_ms) / 1000 : 0;
      if (
        previous &&
        dt > 0 &&
        dt <= 2 &&
        previous.speed != null &&
        sample.speed != null
      )
        distance += ((previous.speed + sample.speed) / 2 / 3.6) * dt;
      return { ...sample, distance, gap: dt > 2 || !!previous && (previous.speed == null || sample.speed == null) };
    });
}
export function mapBounds(points: [number, number][]) {
  if (!points.length) return [0, 0, 1000, 600];
  const xs = points.map(p => p[0]), ys = points.map(p => p[1]);
  const x = Math.min(...xs), y = Math.min(...ys);
  return [x - 80, y - 80, Math.max(1, Math.max(...xs) - x) + 160, Math.max(1, Math.max(...ys) - y) + 160];
}
type Trace = ReturnType<typeof distanceTrace>;
export function tracePoint(rows: Trace, distance: number): [number, number] | null {
  if (distance < 0) return null;
  const after = rows.findIndex(r => r.distance >= distance);
  if (after < 0) return null;
  const b = rows[after], a = rows[Math.max(0, after - 1)];
  if (b.gap || a.x == null || a.y == null || b.x == null || b.y == null) return null;
  const fraction = b.distance === a.distance ? 0 : (distance - a.distance) / (b.distance - a.distance);
  return [a.x + (b.x - a.x) * fraction, a.y + (b.y - a.y) * fraction];
}
export function miniSectors(rows: Trace, reference: Trace, length: number, count = 20) {
  const elapsed = (trace: Trace, distance: number) => {
    const i = trace.findIndex(r => r.distance >= distance);
    if (i < 0) return null;
    const b = trace[i], a = trace[Math.max(0, i - 1)];
    if (b.gap) return null;
    const fraction = b.distance === a.distance ? 0 : (distance - a.distance) / (b.distance - a.distance);
    return a.time_ms + (b.time_ms - a.time_ms) * fraction;
  };
  return Array.from({ length: count }, (_, i) => {
    const start = i * length / count, end = (i + 1) * length / count;
    const a = elapsed(rows, start), b = elapsed(rows, end), c = elapsed(reference, start), d = elapsed(reference, end);
    const p = tracePoint(rows, start), q = tracePoint(rows, end);
    if (!p || !q || a == null || b == null || c == null || d == null || rows.some(r => r.gap && r.distance >= start && r.distance <= end) || reference.some(r => r.gap && r.distance >= start && r.distance <= end)) return null;
    return { points: [p, q], winner: Math.abs((b - a) - (d - c)) < 1 ? "equal" : b - a < d - c ? "driver" : "reference" };
  }).filter((s): s is NonNullable<typeof s> => s != null);
}
export function rolling(values: (number | null)[], n: number) {
  return values.map((_, i) => {
    const valid = values
      .slice(Math.max(0, i - n + 1), i + 1)
      .filter((v): v is number => v != null);
    return valid.length
      ? valid.reduce((a, b) => a + b, 0) / valid.length
      : null;
  });
}
export function chartRows(
  driver: DriverState,
  kind: ChartKind,
  fuel: boolean,
  from: number,
  to: number,
): [number, number | null][] {
  const laps = driver.laps.filter((l) => l.number >= from && l.number <= to);
  if (kind === "sectors")
    return [0, 1, 2].map((i) => {
      const rows = laps
        .map((l) => l.sectors[i])
        .filter((n): n is number => n != null);
      return [
        i + 1,
        rows.length ? rows.reduce((a, b) => a + b, 0) / rows.length : null,
      ];
    });
  if (kind === "stints")
    return driver.stints.map((s) => [
      s.start_lap,
      (s.end_lap ?? driver.lap) - s.start_lap + 1,
    ]);
  return laps.map((l) => [
    kind === "degradation" ? (l.tyre_age ?? l.number) : l.number,
    kind === "position"
      ? l.position
      : kind === "gap" || kind === "leader"
        ? l.gap
        : kind === "speed"
          ? l.speed_trap
          : l.duration == null
            ? null
            : l.duration + (fuel ? l.number * 0.035 : 0),
  ]);
}
export function insightDelta(a: DriverState, b: DriverState) {
  const x = a.prediction?.pace[0],
    y = b.prediction?.pace[0];
  if (!x || !y || a.gap == null || b.gap == null)
    return "Complete more clean laps to compare the pace trend.";
  const advantage = y.value - x.value;
  const gap = a.gap - b.gap;
  return advantage > 0 && gap > 0
    ? `${a.driver.code} is estimated ${advantage.toFixed(2)} s/lap quicker; catch in ~${Math.ceil(gap / advantage)} laps. Basic estimate; traffic and pit stops may change this.`
    : `${a.driver.code} / ${b.driver.code}: pace difference ${advantage.toFixed(2)} s/lap. Forecast bands overlap; positions are uncertain.`;
}
