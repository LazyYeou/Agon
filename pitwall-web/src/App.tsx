import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { useTiming } from "./stream";
import TimingTower from "./TimingTower";
import TrackMap from "./TrackMap";
import Modal from "./Modal";
import RaceRisks from "./RaceRisks";
import type { Session } from "./types/Session";
import type { DriverState } from "./types/DriverState";
import {
  columnNames,
  chartNames,
  getLocal,
  setLocal,
  readColumns,
  readView,
  lapTime,
  num,
  type Column,
  type ChartKind,
} from "./view";
const Charts = lazy(() => import("./Charts"));
const TelemetryPanel = lazy(() => import("./TelemetryPanel"));
const PinnedPlot = lazy(() =>
  import("./Charts").then((m) => ({ default: m.ComparisonPlot })),
);
export default function App() {
  const { state, status, error, setError, samples, send, evaluation } =
    useTiming();
  const [tab, setTab] = useState("Tower");
  const [driver, setDriver] = useState<number | null>(null);
  const [explain, setExplain] = useState<number | null>(null);
  const [columns, setColumns] = useState(() =>
    readColumns(getLocal("agon-columns")),
  );
  const [columnDialog, setColumnDialog] = useState(false);
  const [sessionDialog, setSessionDialog] = useState(false);
  const [interval, setInterval] = useState(false);
  const [year, setYear] = useState(2025);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionLoading, setSessionLoading] = useState(false);
  const [sessionError, setSessionError] = useState("");
  const [sessionSearch, setSessionSearch] = useState("");
  const [opening, setOpening] = useState<number | null>(null);
  const [selected, setSelected] = useState(readView(location.search).drivers);
  const [chart, setChart] = useState<ChartKind>(
    readView(location.search).chart,
  );
  const [pinned, setPinned] = useState<ChartKind | null>(null);
  const [controlDriver, setControlDriver] = useState("all");
  const [scrub, setScrub] = useState<number | null>(null);
  const seekTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const touch = useRef<{ x: number; y: number } | null>(null);
  const views = ["Tower", "Map", "Charts"];
  const moveView = (offset: number) => setTab(current => views[Math.max(0, Math.min(2, views.indexOf(current) + offset))]);
  useEffect(() => {
    setLocal("agon-columns", JSON.stringify(columns));
  }, [columns]);
  useEffect(() => {
    if (!sessionDialog) return;
    const controller = new AbortController();
    setSessionLoading(true);
    setSessionError("");
    setSessions([]);
    fetch(`/api/sessions?year=${year}`, { signal: controller.signal })
      .then(async (r) => {
        const v = await r.json();
        if (!r.ok) throw Error(v.error ?? "Session catalog unavailable");
        return v as Session[];
      })
      .then(setSessions)
      .catch((e) => {
        if (!controller.signal.aborted) setSessionError(String(e));
      })
      .finally(() => {
        if (!controller.signal.aborted) setSessionLoading(false);
      });
    return () => controller.abort();
  }, [sessionDialog, year]);
  useEffect(() => {
    if (state) {
      setSelected((previous) => {
        const valid = previous.filter((n) => state.drivers[n]);
        return valid.length >= 2
          ? valid
          : Object.values(state.drivers)
              .filter((d): d is DriverState => !!d)
              .sort((a, b) => (a.position ?? 99) - (b.position ?? 99))
              .slice(0, 2)
              .map((d) => d.driver.number);
      });
      if (opening === state.session.key) setOpening(null);
    }
  }, [state?.session.key]);
  useEffect(() => {
    if (error) setOpening(null);
  }, [error]);
  useEffect(
    () => () => {
      if (seekTimer.current) clearTimeout(seekTimer.current);
    },
    [],
  );
  const chooseDriver = (n: number) => {
    setDriver(n);
    send({ kind: "subscribe", driver: n });
  };
  const closeDriver = () => {
    setDriver(null);
    send({ kind: "subscribe", driver: null });
  };
  const chooseSession = (key: number) => {
    closeDriver();
    setOpening(key);
    send({ kind: "open", session: key });
    setSessionDialog(false);
    setPinned(null);
  };
  const moveColumn = (column: Column, offset: number) => {
    setColumns((old) => {
      const next = [...old],
        i = next.indexOf(column),
        to = i + offset;
      if (i < 1 || to < 1 || to >= next.length) return old;
      [next[i], next[to]] = [next[to], next[i]];
      return next;
    });
  };
  const all = state
    ? Object.values(state.drivers)
        .filter((d): d is DriverState => !!d)
        .sort((a, b) => (a.position ?? 99) - (b.position ?? 99))
    : [];
  const leader = all[0];
  const leaderPace = leader?.prediction?.pace[0];
  const live = state?.data_status === "live" || state?.data_status === "data delayed";
  const delayed = live && (state?.data_status === "data delayed" || status !== "connected");
  const trained = leader?.prediction && !leader.prediction.model.startsWith("basic-");
  const modelLabel = trained ? `Trained model${leader.prediction?.label.includes("below target") ? " · below target" : ""}` : "Basic estimates";
  const prediction =
    explain != null ? state?.drivers[explain]?.prediction : null;
  const localTime = (time: number) =>
    new Intl.DateTimeFormat(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      timeZoneName: "short",
    }).format(time);
  return (
    <main className="app-shell">
      <header className="app-header">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            a
          </span>
          <div>
            <h1>Agon</h1>
            <span>Race intelligence</span>
          </div>
        </div>
        <div className="header-session">
          <span>{state?.session.event ?? "Timing workspace"}</span>
          <strong>{state?.session.name ?? "Connecting…"}</strong>
        </div>
        <button
          className="session-button"
          onClick={() => setSessionDialog(true)}
        >
          Select session <span aria-hidden="true">⌄</span>
        </button>
        <button
          className="live-button"
          onClick={() => {
            closeDriver();
            send({ kind: "live" });
          }}
        >
          Live
        </button>
        <div className="connection">
          <i className={status === "connected" ? "connected" : ""} />
          <span>{status === "connected" ? "Connected" : status}</span>
        </div>
      </header>
      {error ? (
        <div role="alert" className="error-banner">
          <span>{error}</span>
          <button
            onClick={() => {
              setError("");
              send({ kind: "resync" });
            }}
          >
            Retry
          </button>
        </div>
      ) : null}
      {(status !== "connected" || delayed) && state ? (
        <div role="status" className="delay-banner">
          {live ? "Live data delayed" : "Data delayed"} · keeping the last received state. Source and connection recover automatically.
        </div>
      ) : null}
      {opening != null ? (
        <div role="status" className="delay-banner">
          Loading OpenF1 session {opening}… Historical requests follow the
          source quota and are cached.
        </div>
      ) : null}
      {!state ? (
        <div className="loading-screen">
          <span className="loading-ring" />
          <h2>Opening the timing workspace</h2>
          <p>
            The local strategy lab will load first. Choose a historical session
            to replay real data.
          </p>
        </div>
      ) : (
        <>
          <section className="session-strip">
            <div className="session-state">
              <span
                className={`track-status ${state.track_status.toLowerCase()}`}
              >
                ● {state.track_status.replace("_", " ")}
              </span>
              <strong>
                Lap {state.lap}
                <span> / {state.session.total_laps ?? "—"}</span>
              </strong>
              <span className="source-tag">
                {state.session.synthetic ? "Synthetic demo" : live ? delayed ? "Live data delayed" : "OpenF1 live" : "OpenF1 replay"}
              </span>
            </div>
            <div className="weather">
              <span>
                Track <b>{num(state.weather.track)}°</b>
              </span>
              <span>
                Air <b>{num(state.weather.air)}°</b>
              </span>
              <span>
                Humidity <b>{num(state.weather.humidity, 0)}%</b>
              </span>
              <span>
                Wind{" "}
                <b>
                  {num(state.weather.wind_speed)} m/s ·{" "}
                  {num(state.weather.wind_direction, 0)}°
                </b>
              </span>
              <span>
                Rain{" "}
                <b>
                  {state.weather.rain == null
                    ? "—"
                    : state.weather.rain
                      ? "Yes"
                      : "No"}
                </b>
              </span>
            </div>
          </section>
          <section className="race-insight">
            <span className="insight-icon" aria-hidden="true">
              ◇
            </span>
            <p>
              {leaderPace ? (
                <>
                  <strong>
                    {leader.driver.code} next lap {lapTime(leaderPace.value)}
                  </strong>
                  <span>
                    Range {lapTime(leaderPace.low)}–{lapTime(leaderPace.high)} ·{" "}
                    {leader.prediction?.label}
                  </span>
                </>
              ) : (
                <>
                  <strong>Strategy follows the data</strong>
                  <span>
                    {state.track_status === "RED"
                      ? "Predictions paused during red flag."
                      : "Pace estimates appear after three clean laps in a stint."}
                  </span>
                </>
              )}
              {(/race|sprint/i).test(state.session.name) && <span>
                {leader?.prediction?.safety_car
                  ? `New SC/VSC: ${num(leader.prediction.safety_car.probability * 100, 1)}% within ${leader.prediction.safety_car.horizon_laps} laps · experimental`
                  : "SC/VSC probability unavailable"}
              </span>}
            </p>
            <span className="model-tag">
              {state.track_status === "RED"
                ? `Paused · ${modelLabel}`
                : state.track_status === "SC" ||
                    state.track_status === "VSC" ||
                    state.weather.rain
                  ? `Low confidence · ${modelLabel}`
                  : modelLabel}
            </span>
          </section>
          <nav className="view-tabs" aria-label="Timing views" onKeyDown={e => {
            if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
              e.preventDefault();
              moveView(e.key === "ArrowRight" ? 1 : -1);
              const index = Math.max(0, Math.min(2, views.indexOf(tab) + (e.key === "ArrowRight" ? 1 : -1)));
              e.currentTarget.querySelectorAll("button")[index]?.focus();
            }
          }}>
            {views.map((name) => (
              <button
                key={name}
                aria-pressed={tab === name}
                onClick={() => setTab(name)}
              >
                {name}
              </button>
            ))}
            <span>
              {all.length} drivers · {localTime(state.clock_ms)}
            </span>
          </nav>
          <div className={`workspace view-${tab.toLowerCase()}`}
            onTouchStart={e => {
              const target = e.target as HTMLElement;
              if (!window.matchMedia("(max-width: 760px)").matches || target.closest("button, input, select, canvas, .tower-scroll")) return;
              if (e.touches.length === 1) touch.current = { x: e.touches[0].clientX, y: e.touches[0].clientY };
            }}
            onTouchEnd={e => {
              const start = touch.current;
              touch.current = null;
              if (!start || !e.changedTouches[0]) return;
              const dx = e.changedTouches[0].clientX - start.x, dy = e.changedTouches[0].clientY - start.y;
              if (Math.abs(dx) > 70 && Math.abs(dx) > Math.abs(dy) * 1.5) moveView(dx < 0 ? 1 : -1);
            }}
            onTouchCancel={() => { touch.current = null; }}>
            <section className="tower-panel" hidden={tab === "Charts"}>
              <div className="panel-heading">
                <div>
                  <h2>Timing tower</h2>
                  <p>
                    {/qualifying/i.test(state.session.name)
                      ? "Qualifying · knockout pace"
                      : /practice/i.test(state.session.name)
                        ? "Practice · best laps and stints"
                        : "Race pace and strategy outlook"}
                  </p>
                </div>
                <div className="panel-actions">
                  <button
                    aria-pressed={interval}
                    onClick={() => setInterval(!interval)}
                  >
                    {interval ? "Interval" : "Gap"}
                  </button>
                  <button onClick={() => setColumnDialog(true)}>Columns</button>
                </div>
              </div>
              <TimingTower
                state={state}
                columns={columns}
                interval={interval}
                onSelect={chooseDriver}
                onExplain={setExplain}
              />
            </section>
            <aside className="map-column" hidden={tab === "Charts"}>
              <TrackMap
                state={state}
                onSelect={chooseDriver}
                selected={driver ?? selected[0]}
              />
              <section className="control-panel">
                <div className="panel-heading">
                  <h2>Race control</h2>
                  <label>
                    <span className="sr-only">
                      Filter race control by driver
                    </span>
                    <select
                      value={controlDriver}
                      onChange={(e) => setControlDriver(e.target.value)}
                    >
                      <option value="all">All drivers</option>
                      {all.map((d) => (
                        <option key={d.driver.number} value={d.driver.number}>
                          {d.driver.code}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
                <div className="control-feed">
                  {state.messages
                    .filter(
                      (m) =>
                        controlDriver === "all" || m.driver === +controlDriver,
                    )
                    .slice(-30)
                    .reverse()
                    .map((m, i) => (
                      <article key={`${m.time_ms}-${i}`}>
                        <time>{localTime(m.time_ms).split(" ")[0]}</time>
                        <p>
                          <span className={m.flag?.toLowerCase() ?? ""}>
                            {m.flag ?? m.category}
                          </span>
                          {m.message}
                        </p>
                      </article>
                    ))}
                  {!state.messages.filter(
                    (m) =>
                      controlDriver === "all" || m.driver === +controlDriver,
                  ).length ? (
                    <p className="source-note">
                      No race-control messages at this replay time.
                    </p>
                  ) : null}
                </div>
              </section>
            </aside>
            {tab === "Charts" ? (
              <Suspense fallback={<p>Loading comparison charts…</p>}>
                <Charts
                  state={state}
                  selected={selected}
                  setSelected={setSelected}
                  kind={chart}
                  setKind={setChart}
                  forecasts={evaluation?.forecasts ?? []}
                  report={evaluation?.report ?? null}
                  onPin={() => {
                    setPinned(chart);
                    setTab("Tower");
                  }}
                />
              </Suspense>
            ) : null}
          </div>
          {pinned && tab !== "Charts" ? (
            <section className="pinned-panel">
              <div className="panel-heading">
                <h2>Pinned · {chartNames[pinned]}</h2>
                <button onClick={() => setPinned(null)}>Unpin</button>
              </div>
              <Suspense fallback={<p>Loading chart…</p>}>
                <PinnedPlot state={state} selected={selected} kind={pinned} forecasts={evaluation?.forecasts ?? []} />
              </Suspense>
            </section>
          ) : null}
          <section className="playback" aria-label="Replay controls">
            <div className="playback-main">
              <span className="replay-label">{live ? delayed ? "Delayed" : "Live" : "Replay"}</span>
              <button
                className="play-button"
                aria-label={state.playing ? "Pause replay" : "Play replay"}
                disabled={live}
                onClick={() => send({ kind: "play", playing: !state.playing })}
              >
                {state.playing ? "Ⅱ" : "▶"}
              </button>
              <label>
                <span className="sr-only">Replay speed</span>
                <select
                  aria-label="Replay speed"
                  disabled={live}
                  value={state.speed}
                  onChange={(e) =>
                    send({ kind: "speed", speed: +e.target.value })
                  }
                >
                  {[1, 2, 5, 10, 20].map((v) => (
                    <option key={v} value={v}>
                      {v}×
                    </option>
                  ))}
                </select>
              </label>
              <time>{localTime(scrub ?? state.clock_ms)}</time>
            </div>
            <label className="timeline">
              <span className="sr-only">Replay timeline</span>
              <input
                aria-label="Replay timeline"
                disabled={live}
                type="range"
                min={state.session.start_ms}
                max={state.session.end_ms}
                step="1000"
                value={scrub ?? state.clock_ms}
                onChange={(e) => {
                  const value = +e.target.value;
                  setScrub(value);
                  if (seekTimer.current) clearTimeout(seekTimer.current);
                  seekTimer.current = setTimeout(() => {
                    send({ kind: "seek", time_ms: value });
                    setScrub(null);
                  }, 120);
                }}
              />
            </label>
            <span className="playback-end">
              {localTime(state.session.end_ms)}
            </span>
          </section>
          <footer>
            <span>
              Unofficial · Not affiliated with Formula 1, FIA or any team.
              Non-commercial fan / educational project.
            </span>
            <a href="https://openf1.org" target="_blank" rel="noreferrer">
              Data: OpenF1 · CC BY-NC-SA 4.0
            </a>
          </footer>
          {driver != null && state.drivers[driver] ? (
            <Suspense
              fallback={
                <div className="drawer-loading" role="status">
                  Opening driver telemetry…
                </div>
              }
            >
              <TelemetryPanel
                key={`${state.session.key}-${driver}`}
                state={state}
                number={driver}
                samples={samples?.driver === driver ? samples.samples : []}
                onClose={closeDriver}
              />
            </Suspense>
          ) : null}
        </>
      )}
      {sessionDialog ? (
        <Modal label="Select session" onClose={() => setSessionDialog(false)}>
          <div className="panel-heading">
            <h2>Select session</h2>
            <button
              aria-label="Close session selector"
              onClick={() => setSessionDialog(false)}
            >
              ✕
            </button>
          </div>
          <button className="demo-choice" onClick={() => chooseSession(0)}>
            <strong>Strategy lab</strong>
            <span>Synthetic demo · deterministic test data</span>
          </button>
          <div className="session-search">
            <label>
              Season
              <select
                aria-label="Season"
                value={year}
                onChange={(e) => setYear(+e.target.value)}
              >
                {Array.from(
                  { length: Math.max(new Date().getFullYear(), 2026) - 2022 },
                  (_, i) => 2023 + i,
                )
                  .reverse()
                  .map((y) => (
                    <option key={y} value={y}>
                      {y}
                    </option>
                  ))}
              </select>
            </label>
            <label>
              Find event
              <input
                placeholder="Circuit or session"
                value={sessionSearch}
                onChange={(e) => setSessionSearch(e.target.value)}
              />
            </label>
          </div>
          <p className="source-note">
            Historical sessions from 2023 onward. Live ingest needs sponsor
            access configured on the server.
          </p>
          {sessionLoading ? <p role="status">Loading OpenF1 catalog…</p> : null}
          {sessionError ? <p role="alert">{sessionError}</p> : null}
          <div className="session-list">
            {sessions
              .filter((s) =>
                `${s.event} ${s.name} ${s.circuit}`
                  .toLowerCase()
                  .includes(sessionSearch.toLowerCase()),
              )
              .sort((a, b) => b.start_ms - a.start_ms)
              .map((s) => (
                <button key={s.key} onClick={() => chooseSession(s.key)}>
                  <span>
                    <strong>{s.event}</strong>
                    <small>
                      {s.circuit} · {new Date(s.start_ms).toLocaleDateString()}
                    </small>
                  </span>
                  <b>{s.name}</b>
                </button>
              ))}
            {!sessionLoading && !sessions.length && !sessionError ? (
              <p>No sessions available for this year.</p>
            ) : null}
          </div>
        </Modal>
      ) : null}
      {columnDialog ? (
        <Modal
          label="Timing tower columns"
          onClose={() => setColumnDialog(false)}
        >
          <div className="panel-heading">
            <h2>Timing tower columns</h2>
            <button onClick={() => setColumnDialog(false)}>Done</button>
          </div>
          <p>
            Changes are saved on this device. Driver identity stays visible.
          </p>
          <div className="column-list">
            {[
              ...columns,
              ...Object.keys(columnNames).filter(
                (c) => !columns.includes(c as Column),
              ),
            ].map((c) => (
              <div key={c}>
                <label>
                  <input
                    aria-label={`Show ${columnNames[c as Column]}`}
                    type="checkbox"
                    checked={columns.includes(c as Column)}
                    disabled={c === "driver"}
                    onChange={(e) =>
                      setColumns((old) =>
                        e.target.checked
                          ? [...old, c as Column]
                          : old.filter((v) => v !== c),
                      )
                    }
                  />
                  {columnNames[c as Column]}
                </label>
                {columns.includes(c as Column) && c !== "driver" ? (
                  <span>
                    <button
                      aria-label={`Move ${columnNames[c as Column]} left`}
                      disabled={columns.indexOf(c as Column) <= 1}
                      onClick={() => moveColumn(c as Column, -1)}
                    >
                      ←
                    </button>
                    <button
                      aria-label={`Move ${columnNames[c as Column]} right`}
                      disabled={
                        columns.indexOf(c as Column) === columns.length - 1
                      }
                      onClick={() => moveColumn(c as Column, 1)}
                    >
                      →
                    </button>
                  </span>
                ) : null}
              </div>
            ))}
          </div>
          <button
            onClick={() => setColumns(Object.keys(columnNames) as Column[])}
          >
            Reset layout
          </button>
        </Modal>
      ) : null}
      {explain != null ? (
        <Modal label="Prediction explanation" onClose={() => setExplain(null)}>
          <div className="panel-heading">
            <h2>{state?.drivers[explain]?.driver.code} · Forecast factors</h2>
            <button onClick={() => setExplain(null)}>Close</button>
          </div>
          {prediction ? (
            <>
              <p className="pill">
                {prediction.label} · {prediction.status}
              </p>
              <p>{prediction.insight}</p>
              {state && (/race|sprint/i).test(state.session.name) && <RaceRisks prediction={prediction} drivers={state.drivers} />}
              <ol>
                {prediction.factors.map((factor, i) => (
                  <li key={i}>{factor}</li>
                ))}
              </ol>
              <p className="source-note">
                Model {prediction.model} · issued after lap {prediction.lap}.
                Confidence {Math.round(prediction.confidence * 100)}% is an
                uncalibrated estimate. Displayed bands show uncertainty;
                accuracy requires historical backtesting.
              </p>
            </>
          ) : (
            <p>Not enough clean lap data to issue a forecast.</p>
          )}
        </Modal>
      ) : null}
    </main>
  );
}
