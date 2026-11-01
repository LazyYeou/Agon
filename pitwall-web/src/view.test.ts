import { describe, it, expect } from "vitest";
import { readView, readColumns, mergeDelta, lapTime } from "./view";
describe("view boundaries", () => {
  it("recovers broken preferences without removing required driver identity", () => {
    expect(readColumns("{broken")).toContain("driver");
    expect(readColumns('["pace","pace","unknown"]')).toEqual([
      "driver",
      "pace",
    ]);
  });
  it("rejects invalid share queries and limits comparison to six unique drivers", () => {
    const view = readView(
      "?session=-1&chart=unknown&drivers=1,1,4,16,81,63,44,14&from=-20",
    );
    expect(view.session).toBe(0);
    expect(view.chart).toBe("pace");
    expect(view.drivers).toEqual([1, 4, 16, 81, 63, 44]);
    expect(view.from).toBe(1);
  });
  it("merges changed fields without erasing other drivers and supports null", () => {
    expect(
      mergeDelta(
        { drivers: { 1: { gap: 3, lap: 4 }, 4: { lap: 5 } } },
        { drivers: { 1: { gap: null } } },
      ),
    ).toEqual({ drivers: { 1: { gap: null, lap: 4 }, 4: { lap: 5 } } });
  });
  it("formats lap times without NaN or zero for missing input", () => {
    expect(lapTime(null)).toBe("—");
    expect(lapTime(91.743)).toBe("1:31.743");
  });
});
