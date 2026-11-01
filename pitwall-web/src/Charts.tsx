import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import type { PaceForecast } from "./types/PaceForecast";
import type { AccuracyReport } from "./types/AccuracyReport";
import type { Snapshot } from "./types/Snapshot";
import type { DriverState } from "./types/DriverState";
import type { ChartOption } from "./EChart";
import { insightDelta } from "./analysis";
import { comparisonSeries, teammateDash } from "./comparison";
import { chartNames, num, readView, type ChartKind } from "./view";
const EChart = lazy(() => import("./EChart"));
export function ComparisonPlot({
  state,
  selected,
  kind,
  fuel = false,
  from = 1,
  to = 500,
  onReady,
  forecasts = [],
}: {
  state: Snapshot;
  selected: number[];
  kind: ChartKind;
  fuel?: boolean;
  from?: number;
  to?: number;
  forecasts?: PaceForecast[];
  onReady?: (chart: import("echarts/core").ECharts) => void;
}) {
  const signature = Object.values(state.drivers)
    .map((d) => `${d?.lap}-${d?.prediction?.lap}`)
    .join(",");
  const option = useMemo<ChartOption>(() => {
    const drivers = selected.flatMap((n) =>
      state.drivers[n] ? [state.drivers[n]!] : [],
    );
    const series = comparisonSeries(drivers, kind, fuel, from, to, forecasts);
    return {
      grid: { left: 56, right: 20, top: 32, bottom: 64 },
      legend: { show: false },
      axisPointer: { link: [{ xAxisIndex: "all" }] },
      tooltip: { trigger: "axis", axisPointer: { type: "cross" } },
      xAxis: {
        type: "value",
        name:
          kind === "degradation"
            ? "Tyre age (laps)"
            : kind === "sectors"
              ? "Sector"
              : "Lap",
        min: kind === "stints" ? 0 : undefined,
        nameLocation: "middle",
        nameGap: 32,
        axisLine: { lineStyle: { color: "#44566d" } },
        splitLine: { lineStyle: { color: "#223044" } },
      },
      yAxis:
        kind === "stints"
          ? { type: "category", data: drivers.map((d) => d.driver.code) }
          : {
              type: "value",
              scale: true,
              inverse: kind === "position",
              name:
                kind === "position"
                  ? "Position"
                  : kind === "speed"
                    ? "km/h"
                    : kind === "gap" || kind === "leader" || kind === "sectors"
                      ? "Seconds Δ"
                      : "Seconds",
              splitLine: { lineStyle: { color: "#223044" } },
            },
      dataZoom: [
        { type: "inside" },
        {
          type: "slider",
          height: 14,
          bottom: 8,
          borderColor: "#29364a",
          fillerColor: "#75baff24",
        },
      ],
      series,
    };
  }, [
    signature,
    selected.join(","),
    kind,
    fuel,
    from,
    to,
    state.session.key,
    forecasts,
    state.revision,
  ]);
  return (
    <Suspense fallback={<p className="source-note">Loading chart renderer…</p>}>
      <EChart
        option={option}
        group={kind === "degradation" ? "agon-tyre-age-crosshair" : kind === "sectors" ? "agon-sector-crosshair" : "agon-lap-crosshair"}
        onReady={onReady}
        label={`${chartNames[kind]}: ${kind === "accuracy" ? `${forecasts.filter(p => selected.includes(p.driver) && p.target_lap === p.issued_lap + 1 && p.target_lap >= from && p.target_lap <= to).length} recorded forecasts with original bands and matched actuals` : "compares completed lap data and labelled estimates"}. Drag or scroll to zoom.`}
      />
    </Suspense>
  );
}
export default function Charts({
  state,
  selected,
  setSelected,
  kind,
  setKind,
  onPin,
  forecasts,
  report,
}: {
  state: Snapshot;
  selected: number[];
  setSelected: (n: number[]) => void;
  kind: ChartKind;
  setKind: (k: ChartKind) => void;
  onPin: () => void;
  forecasts: PaceForecast[];
  report: AccuracyReport | null;
}) {
  const [fuel, setFuel] = useState(false);
  const [from, setFrom] = useState(readView(location.search).from);
  const [to, setTo] = useState(readView(location.search).to);
  const [lastN, setLastN] = useState(5);
  const [share, setShare] = useState("");
  const [strategy, setStrategy] = useState<{
    undercut: { value: number; low: number; high: number } | null;
    overcut: { value: number; low: number; high: number } | null;
    assumptions: string[];
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const chart = useRef<import("echarts/core").ECharts | null>(null);
  const [backtest, setBacktest] = useState<{
    status: string;
    model?: string;
    pace_samples?: number;
    pace_mae?: number;
    pace_1_to_5_mae?: number;
    pit_candidates?: number;
    pit_forecast_coverage?: number;
    pit_samples?: number;
    pit_accuracy?: number;
    limitations?: string[];
  } | null>(null);
  useEffect(() => {
    const abort = new AbortController();
    fetch("/api/accuracy", { signal: abort.signal })
      .then((r) => r.json())
      .then(setBacktest)
      .catch(() => {});
    return () => abort.abort();
  }, []);
  const drivers = Object.values(state.drivers)
    .filter((d): d is DriverState => !!d)
    .sort((a, b) => (a.position ?? 99) - (b.position ?? 99));
  const toggle = (n: number) => {
    if (selected.includes(n)) {
      if (selected.length > 2) setSelected(selected.filter((v) => v !== n));
    } else if (selected.length < 6) setSelected([...selected, n]);
  };
  const shareView = () => {
    const p = new URLSearchParams({
      session: String(state.session.key),
      drivers: selected.join(","),
      chart: kind,
      from: String(from),
      to: String(to),
    });
    const url = `${location.origin}/?${p}`;
    history.replaceState(null, "", url);
    setShare(url);
    navigator.clipboard?.writeText(url).catch(() => {});
  };
  const download = () => {
    if (!chart.current) return;
    const link = document.createElement("a");
    link.download = `agon-${kind}-lap-${state.lap}.png`;
    link.href = chart.current.getDataURL({
      type: "png",
      pixelRatio: 2,
      backgroundColor: "#151f2e",
    });
    link.click();
  };
  async function calculate() {
    setBusy(true);
    setError("");
    try {
      const response = await fetch(
        `/api/strategy?${new URLSearchParams({ session: String(state.session.key), time_ms: String(state.clock_ms), a: String(selected[0]), b: String(selected[1]) })}`,
      );
      const data = await response.json();
      if (!response.ok) throw Error(data.error ?? "Calculation unavailable");
      setStrategy(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="charts-panel">
      <div className="panel-heading">
        <div>
          <h2>Driver comparison</h2>
          <p>Select 2–6 drivers. New laps update automatically.</p>
        </div>
        <span className="pill">{selected.length} / 6 drivers</span>
      </div>
      <div className="driver-picker">
        {drivers.map((d) => (
          <button
            key={d.driver.number}
            aria-pressed={selected.includes(d.driver.number)}
            disabled={
              !selected.includes(d.driver.number) && selected.length >= 6
            }
            onClick={() => toggle(d.driver.number)}
            style={
              { "--team-color": `#${d.driver.color}` } as React.CSSProperties
            }
          >
            {d.driver.code}
          </button>
        ))}
      </div>
      <div className="chart-toolbar">
        <label>
          Chart type
          <select
            aria-label="Chart type"
            value={kind}
            onChange={(e) => setKind(e.target.value as ChartKind)}
          >
            {Object.entries(chartNames).map(([key, name]) => (
              <option key={key} value={key}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label>
          From lap
          <input
            type="number"
            min="1"
            max={Math.max(state.lap, 1)}
            value={from}
            onChange={(e) =>
              setFrom(Math.max(1, Math.min(Number(e.target.value) || 1, to)))
            }
          />
        </label>
        <label>
          To lap
          <input
            type="number"
            min={from}
            max="500"
            value={to}
            onChange={(e) =>
              setTo(
                Math.max(from, Math.min(500, Number(e.target.value) || 500)),
              )
            }
          />
        </label>
        {kind === "sectors" ? (
          <label>
            Last N laps
            <input
              type="number"
              min="1"
              max="50"
              value={lastN}
              onChange={(e) =>
                setLastN(Math.min(50, Math.max(1, +e.target.value)))
              }
            />
          </label>
        ) : null}
        <label className="inline-check">
          <input
            type="checkbox"
            checked={fuel}
            onChange={(e) => setFuel(e.target.checked)}
          />
          Fuel correction (approx.)
        </label>
        <button onClick={onPin}>Pin chart</button>
        <button onClick={download}>Export PNG</button>
        <button onClick={shareView}>Share view</button>
      </div>
      {share ? (
        <label className="share-link">
          Share link
          <input
            aria-label="Share link"
            readOnly
            value={share}
            onFocus={(e) => e.currentTarget.select()}
          />
        </label>
      ) : null}
      <ComparisonPlot
        state={state}
        selected={selected}
        kind={kind}
        fuel={fuel}
        forecasts={forecasts}
        from={kind === "sectors" ? Math.max(from, state.lap - lastN + 1) : from}
        to={to}
        onReady={(c) => {
          chart.current = c;
        }}
      />
      {kind === "accuracy" ? (
        <section className="accuracy-panel">
          <h3>Forecast accuracy</h3>
          <p>
            Playback: {report?.pace_samples ?? 0} scored forecasts · MAE{" "}
            {num(report?.pace_mae, 3)} s · band coverage{" "}
            {report?.band_coverage == null
              ? "—"
              : `${Math.round(report.band_coverage * 100)}%`}
            . Seek starts a new evaluation run.
          </p>
          <p>
            Race-held-out backtest:{" "}
            {backtest?.status === "measured"
              ? `${backtest.pace_samples} clean next-lap samples · MAE ${num(backtest.pace_mae, 3)} s · model ${backtest.model}`
              : "No measured report available."}
          </p>
          <p>Next 1–5 laps MAE: {num(backtest?.pace_1_to_5_mae, 3)} s.</p>
          <p>
            Pit accuracy at 10-lap horizon:{" "}
            {backtest?.pit_accuracy == null
              ? "Not scored"
              : `${num(backtest.pit_accuracy * 100, 1)}% (${backtest.pit_samples} scored / ${backtest.pit_candidates ?? "?"} eligible stops; forecast coverage ${num(backtest.pit_forecast_coverage == null ? null : backtest.pit_forecast_coverage * 100, 1)}%)`}
            . Finish target remains unverified without scheduled-distance
            metadata.
          </p>
          {backtest?.limitations?.map((l, i) => (
            <p className="source-note" key={i}>
              {l}
            </p>
          ))}
        </section>
      ) : null}
      <div className="chart-legend">
        {selected.flatMap((n) =>
          state.drivers[n]
            ? [
                <span key={n}>
                  <i
                    style={{
                      background: `#${state.drivers[n]!.driver.color}`,
                      borderTop: teammateDash(selected.flatMap(v => state.drivers[v] ? [state.drivers[v]!] : []), state.drivers[n]!) ? "2px dashed white" : undefined,
                    }}
                  />
                  {state.drivers[n]!.driver.code}
                </span>,
              ]
            : [],
        )}
      </div>
      <p className="source-note">
        {kind === "speed"
          ? "Speed-trap data appears when the source supplies a measured trap value."
          : kind === "accuracy"
            ? "Solid = actual; dashed = forecast; shaded rectangles = p10–p90. Forecasts start after issue lap; completed backtest scores are separate."
            : kind === "stints"
              ? "Thick = actual compound stint; dashed = predicted current-stint continuation; translucent range = pit-window uncertainty. Future compound is unknown."
              : kind === "pace"
                ? "Grey points mark pit, SC/VSC and red-flag outliers; coloured lines and rolling means use clean laps. Fuel correction is approximate."
              : "Clean-lap trends are approximate. Pit/safety-car laps remain visible in the driver history and are excluded from the pace fit."}
      </p>
      <section className="strategy-panel">
        <div className="panel-heading">
          <div>
            <h3>Undercut / overcut</h3>
            <p>
              {selected
                .slice(0, 2)
                .map((n) => state.drivers[n]?.driver.code)
                .join(" vs ")}
            </p>
          </div>
          <button disabled={busy} onClick={calculate}>
            {busy ? "Calculating…" : "Calculate strategy"}
          </button>
        </div>
        {selected.length >= 2 &&
        state.drivers[selected[0]] &&
        state.drivers[selected[1]] ? (
          <p>
            {insightDelta(
              state.drivers[selected[0]]!,
              state.drivers[selected[1]]!,
            )}
          </p>
        ) : null}
        {error ? <p role="alert">{error}</p> : null}
        {strategy ? (
          <>
            <div className="strategy-results">
              {(["undercut", "overcut"] as const).map((key) => (
                <p key={key}>
                  {key === "undercut" ? "Undercut" : "Overcut"} advantage
                  <strong>
                    {strategy[key]
                      ? `${num(strategy[key]!.value, 2)} s`
                      : "Unavailable"}
                  </strong>
                  <small>
                    {strategy[key]
                      ? `${num(strategy[key]!.low, 2)} to ${num(strategy[key]!.high, 2)} s · basic estimate`
                      : "Needs valid pace and pit-loss data"}
                  </small>
                </p>
              ))}
            </div>
            {strategy.assumptions.map((a, i) => (
              <p className="source-note" key={i}>
                {a}
              </p>
            ))}
          </>
        ) : (
          <p className="source-note">
            Recompute at the displayed replay time. Results include assumptions
            and uncertainty.
          </p>
        )}
      </section>
    </section>
  );
}
