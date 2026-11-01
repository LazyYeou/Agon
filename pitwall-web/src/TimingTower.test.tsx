import { expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import TimingTower from "./TimingTower";
import type { Snapshot } from "./types/Snapshot";
it("uses the phase-aware qualifying need band rather than a bare observed cutoff", () => {
  const state = { session: {name: "Qualifying"}, qualifying_phase: 2,
    drivers: { 1: { driver: {number: 1, team: "A", code: "AAA", color: "ff0000"},
      stints: [], laps: [{number: 1, duration: 90}], position: 11, interval: null, gap: null,
      prediction: { pace: [], qualifying_need: {value: .25, low: .1, high: .4} } } },
  } as unknown as Snapshot;
  const markup = renderToStaticMarkup(<TimingTower state={state} columns={["gap"]} interval={false} onSelect={() => {}} onExplain={() => {}} />);
  expect(markup).toMatch(/Needs 0.100.0.400 s/);
  state.qualifying_phase = null;
  expect(renderToStaticMarkup(<TimingTower state={state} columns={["gap"]} interval={false} onSelect={() => {}} onExplain={() => {}} />)).not.toContain("Needs");
});

it("shows current partial sectors while last lap stays the last completed lap", () => {
  const state = {
    session: { name: "Race" },
    drivers: { 1: {
      driver: { number: 1, team: "A", code: "AAA", color: "ff0000" },
      stints: [], position: 1,
      laps: [{ number: 4, duration: 90.123, sectors: [30, 31, 29.123], segments: [] }],
      partial_lap: { number: 5, duration: null, sectors: [28.456, null, null], segments: [2051] },
      prediction: null,
    } },
  } as unknown as Snapshot;
  const markup = renderToStaticMarkup(<TimingTower state={state} columns={["last", "sectors"]} interval={false} onSelect={() => {}} onExplain={() => {}} />);
  expect(markup).toContain("1:30.123");
  expect(markup).toMatch(/Lap 5[^>]*>28\.456<\/span>/);
  expect(markup).toContain('class="purple-bg"');
  expect(markup).not.toMatch(/>31\.000<\/span>/);
  expect(state.drivers[1]!.laps).toHaveLength(1);
});

