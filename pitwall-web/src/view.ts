export const columnNames = {
  driver: "Driver",
  gap: "Gap / interval",
  last: "Last / best lap",
  sectors: "Sectors",
  tyre: "Tyre / stint",
  pit: "Pit stops",
  pace: "Predicted pace",
  window: "Pit window",
  health: "Tyre health",
  finish: "Projected finish",
} as const;
export type Column = keyof typeof columnNames;
export const chartNames = {
  pace: "Race pace",
  degradation: "Tyre degradation",
  gap: "Gap evolution",
  leader: "Gap to leader",
  sectors: "Sector deltas",
  position: "Position history",
  stints: "Stint strategy",
  speed: "Speed traps",
  accuracy: "Prediction vs actual",
} as const;
export type ChartKind = keyof typeof chartNames;
export function readColumns(raw: string | null): Column[] {
  try {
    const value: unknown = JSON.parse(raw ?? "null");
    if (!Array.isArray(value)) return Object.keys(columnNames) as Column[];
    return [
      "driver",
      ...new Set(
        value.filter(
          (v): v is Column =>
            typeof v === "string" && v in columnNames && v !== "driver",
        ),
      ),
    ];
  } catch {
    return Object.keys(columnNames) as Column[];
  }
}
export function readView(search: string) {
  const p = new URLSearchParams(search);
  const int = (key: string, fallback: number, min: number, max: number) => {
    const value = Number(p.get(key));
    return p.has(key) && Number.isInteger(value) && value >= min && value <= max
      ? value
      : fallback;
  };
  const drivers = [
    ...new Set(
      (p.get("drivers") ?? "1,4")
        .split(",")
        .map(Number)
        .filter((n) => Number.isInteger(n) && n > 0 && n < 1000),
    ),
  ].slice(0, 6);
  return {
    session: int("session", 0, 0, 9999999),
    chart: (p.get("chart") && p.get("chart")! in chartNames
      ? p.get("chart")
      : "pace") as ChartKind,
    drivers: drivers.length >= 2 ? drivers : [1, 4],
    from: int("from", 1, 1, 500),
    to: int("to", 500, 1, 500),
  };
}
export function mergeDelta<T>(original: T, patch: unknown): T {
  if (patch === null || typeof patch !== "object" || Array.isArray(patch))
    return patch as T;
  const result: Record<string, unknown> = {
    ...(original as Record<string, unknown>),
  };
  for (const [key, value] of Object.entries(patch))
    result[key] = mergeDelta(result[key], value);
  return result as T;
}
export function lapTime(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds <= 0) return "—";
  const ms = Math.round(seconds * 1000);
  return `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}.${String(ms % 1000).padStart(3, "0")}`;
}
export const num = (value: number | null | undefined, digits = 1) =>
  value == null || !Number.isFinite(value) ? "—" : value.toFixed(digits);
export const tyreColor = (compound: string) =>
  ({
    SOFT: "#ff7381",
    MEDIUM: "#ffd478",
    HARD: "#e7edf4",
    INTERMEDIATE: "#72d5aa",
    WET: "#7bbcff",
  })[compound] ?? "#a5b5c9";
export function getLocal(key: string) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
export function setLocal(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* Browser storage may be disabled; the current view still works. */
  }
}
