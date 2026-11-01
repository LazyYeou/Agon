import { afterEach, expect, it, vi } from "vitest";
import { fetchLapTelemetry } from "./telemetry";
import type { Session } from "./types/Session";
import type { Lap } from "./types/Lap";
import type { TelemetrySample } from "./types/TelemetrySample";

const session: Session = { key: 42, year: 2025, event: "Test", name: "Race", circuit: "Test", country: "Test", start_ms: 0, end_ms: 500000, total_laps: null, synthetic: false };
const lap: Lap = { number: 4, time_ms: 410000, duration: 400, speed_trap: null, sectors: [], segments: [], pit_out: false, pit_in: false, compound: "MEDIUM", tyre_age: 4, position: 1, gap: 0, interval: 0, status: "GREEN" };
const sample = (time_ms: number, speed = 50): TelemetrySample => ({ time_ms, speed, rpm: null, gear: null, throttle: null, brake: null, drs: null, x: null, y: null });
afterEach(() => vi.unstubAllGlobals());

it("fetches all 400 seconds in sequential bounded requests and deduplicates shared endpoints", async () => {
  let release!: (response: Response) => void;
  const first = new Promise<Response>(resolve => { release = resolve; });
  const request = vi.fn()
    .mockReturnValueOnce(first)
    .mockResolvedValueOnce(Response.json([sample(370000), sample(190000)]))
    .mockResolvedValueOnce(Response.json([sample(370000, 60), sample(410000)]));
  vi.stubGlobal("fetch", request);
  const controller = new AbortController();
  const result = fetchLapTelemetry(session, 1, lap, controller.signal);
  expect(request).toHaveBeenCalledTimes(1);
  release(Response.json([sample(10000), sample(190000)]));
  const rows = await result;
  expect(request.mock.calls.map(([url]) => {
    const query = new URL(String(url), "http://localhost").searchParams;
    expect(query.get("session")).toBe("42");
    expect(query.get("driver")).toBe("1");
    return [Number(query.get("from")), Number(query.get("to"))];
  })).toEqual([[10000, 190000], [190000, 370000], [370000, 410000]]);
  expect(request.mock.calls.every(([, init]) => init.signal === controller.signal)).toBe(true);
  expect(rows.map(row => row.time_ms)).toEqual([10000, 190000, 370000, 410000]);
  expect(rows[2].speed).toBe(60);
});

it("does not start the next chunk when the selection is aborted", async () => {
  let release!: (response: Response) => void;
  const request = vi.fn(() => new Promise<Response>(resolve => { release = resolve; }));
  vi.stubGlobal("fetch", request);
  const controller = new AbortController();
  const result = fetchLapTelemetry(session, 1, lap, controller.signal);
  controller.abort();
  release(Response.json([sample(10000)]));
  await expect(result).rejects.toMatchObject({ name: "AbortError" });
  expect(request).toHaveBeenCalledTimes(1);
});

it("preserves the telemetry error and stops requesting chunks after a failed response", async () => {
  const request = vi.fn().mockResolvedValue(Response.json({ error: "Source unavailable" }, { status: 503 }));
  vi.stubGlobal("fetch", request);
  await expect(fetchLapTelemetry(session, 1, lap, new AbortController().signal)).rejects.toThrow("Source unavailable");
  expect(request).toHaveBeenCalledTimes(1);
});
