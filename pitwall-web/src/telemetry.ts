import type { Session } from "./types/Session";
import type { Lap } from "./types/Lap";
import type { TelemetrySample } from "./types/TelemetrySample";

export async function fetchLapTelemetry(session: Session, driver: number, lap: Lap | undefined, signal: AbortSignal): Promise<TelemetrySample[]> {
  if (!lap?.duration || lap.duration < 0 || !Number.isFinite(lap.duration)) return [];
  const samples = new Map<number, TelemetrySample>();
  let from = Math.max(session.start_ms, Math.floor(lap.time_ms - lap.duration * 1000));
  while (from < lap.time_ms) {
    signal.throwIfAborted();
    const to = Math.min(from + 180000, lap.time_ms);
    const response = await fetch(
      `/api/telemetry?${new URLSearchParams({ session: String(session.key), driver: String(driver), from: String(from), to: String(to) })}`,
      { signal },
    );
    const value = await response.json();
    signal.throwIfAborted();
    if (!response.ok) throw Error(value.error ?? "Telemetry unavailable");
    for (const sample of value as TelemetrySample[]) samples.set(sample.time_ms, sample);
    from = to;
  }
  return [...samples.values()].sort((a, b) => a.time_ms - b.time_ms);
}
