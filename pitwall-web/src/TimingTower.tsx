import type { Snapshot } from "./types/Snapshot";
import type { DriverState } from "./types/DriverState";
import { columnNames, lapTime, num, tyreColor, type Column } from "./view";
const best = (d: DriverState) =>
  Math.min(...d.laps.flatMap((l) => (l.duration == null ? [] : [l.duration])));
export default function TimingTower({
  state,
  columns,
  interval,
  onSelect,
  onExplain,
}: {
  state: Snapshot;
  columns: Column[];
  interval: boolean;
  onSelect: (n: number) => void;
  onExplain: (n: number) => void;
}) {
  const drivers = Object.values(state.drivers)
    .filter((d): d is DriverState => !!d)
    .sort((a, b) => (a.position ?? 99) - (b.position ?? 99));
  const fastest = Math.min(...drivers.map(best));
  const qualifying = /qualifying/i.test(state.session.name);
  const cell = (column: Column, d: DriverState) => {
    const last = d.laps.at(-1);
    const stint = d.stints.at(-1);
    const p = d.prediction;
    const pace = p?.pace[0];
    const sectors=d.partial_lap??last;
    switch (column) {
      case "driver":
        return (
          <button
            className="driver-button"
            aria-label={`Open telemetry for ${d.driver.code}`}
            onClick={() => onSelect(d.driver.number)}
          >
            <span
              className="team-line"
              style={{ background: `#${d.driver.color}` }}
            />
            <span className="driver-code">{d.driver.code}</span>
            <span className="car-number">{d.driver.number}</span>
            <small>{d.driver.team}</small>
          </button>
        );
      case "gap":
        return (
          <span
            className={
              d.interval != null && d.interval < 1 && d.position !== 1
                ? "drs-gap"
                : ""
            }
          >
            {d.position === 1
              ? "Leader"
              : `+${num(interval ? d.interval : d.gap, 3)}`}
            <small>
              {d.interval != null && d.interval < 1 && d.position !== 1
                ? "DRS range"
                : qualifying && state.qualifying_phase != null && p?.qualifying_need
                  ? `Needs ${num(p.qualifying_need.low, 3)}–${num(p.qualifying_need.high, 3)} s · Q${state.qualifying_phase} estimate`
                  : qualifying
                    ? "Qualifying need unavailable"
                  : "seconds"}
            </small>
          </span>
        );
      case "last":
        return (
          <span
            className={
              last?.duration === fastest
                ? "purple"
                : last?.duration === best(d)
                  ? "personal-best"
                  : ""
            }
          >
            {lapTime(last?.duration)}
            <small>
              {last?.duration === fastest
                ? "◆ Fastest · "
                : last?.duration === best(d)
                  ? "↑ PB · "
                  : ""}
              {lapTime(best(d))}
            </small>
          </span>
        );
      case "sectors":
        return (
          <span className="sector-times">
            {[0, 1, 2].map((i) => (
              <span key={i} title={`Lap ${sectors?.number??'—'} · best sector ${i+1}: ${num(Math.min(...d.laps.flatMap(l=>l.sectors[i]==null?[]:[l.sectors[i]!])),3)} s`}>{num(sectors?.sectors[i], 3)}</span>
            ))}
            <span className="mini-sectors">
              {sectors?.segments.length ? (
                sectors.segments.map((v, i) => (
                  <i
                    key={i}
                    className={
                      v === 2051
                        ? "purple-bg"
                        : v === 2049
                          ? "green-bg"
                          : "yellow-bg"
                    }
                  />
                ))
              ) : (
                <small>Mini-sectors unavailable</small>
              )}
            </span>
          </span>
        );
      case "tyre":
        return (
          <span className="tyre-cell">
            <b
              className="compound"
              style={{ color: tyreColor(stint?.compound ?? "") }}
            >
              {stint?.compound[0] ?? "?"}
            </b>
            <span>
              {d.tyre_age ?? "—"} L
              <small>
                S{stint?.number ?? "—"} ·{" "}
                {stint?.is_new == null
                  ? "unknown"
                  : stint.is_new
                    ? "new"
                    : "used"}
              </small>
            </span>
          </span>
        );
      case "pit":
        return (
          <span>
            {d.in_pit ? (
              <b className="pit-tag">In pit</b>
            ) : (
              `${d.pits.length} stops`
            )}
            <small>
              {d.pits.at(-1)
                ? `${num(d.pits.at(-1)?.lane_duration)} s lane`
                : "—"}
            </small>
          </span>
        );
      case "pace":
        return (
          <button
            className="forecast-cell"
            onClick={() => onExplain(d.driver.number)}
            title={p?.label ?? "Waiting for clean laps"}
          >
            {pace ? (
              <>
                {lapTime(pace.value)}
                <small>±{num((pace.high - pace.low) / 2, 2)} s</small>
              </>
            ) : (
              <>
                <span className="muted">—</span>
                <small>{p?.status ?? "Waiting for laps"}</small>
              </>
            )}
          </button>
        );
      case "window":
        return (
          <button
            className="forecast-cell"
            onClick={() => onExplain(d.driver.number)}
          >
            {p?.pit_window ? (
              <>
                L{p.pit_window.from_lap}–{p.pit_window.to_lap}
                <small>
                  {p.pit_window.probability > 0
                    ? `${Math.round(p.pit_window.probability * 100)}%`
                    : "Uncalibrated"}
                </small>
              </>
            ) : (
              <>
                <span className="muted">—</span>
                <small>
                  {p?.status === "paused" ? "Paused" : "Needs stint history"}
                </small>
              </>
            )}
          </button>
        );
      case "health":
        return (
          <button
            className="forecast-cell"
            onClick={() => onExplain(d.driver.number)}
          >
            {p?.tyre_health ? (
              <>
                <span className="health-track">
                  <i style={{ width: `${p.tyre_health.value * 100}%` }} />
                </span>
                <small>
                  {Math.round(p.tyre_health.low * 100)}–
                  {Math.round(p.tyre_health.high * 100)}% life
                </small>
              </>
            ) : (
              <span className="muted">—</span>
            )}
            {p?.laps_to_cliff && p.laps_to_cliff.low < 3 ? (
              <small className="warning">⚠ Cliff risk</small>
            ) : null}
          </button>
        );
      case "finish": {
        const pos = p?.finish.reduce<[number, number] | null>(
          (a, b) => (!a || b[1] > a[1] ? b : a),
          null,
        );
        return (
          <button
            className="forecast-cell"
            onClick={() => onExplain(d.driver.number)}
          >
            {pos ? (
              <>
                P{pos[0]}
                <small>{Math.round(pos[1] * 100)}% · estimate</small>
              </>
            ) : (
              <span className="muted">—</span>
            )}
          </button>
        );
      }
    }
  };
  return (
    <div className="tower-scroll">
      <table className="timing-table">
        <caption className="sr-only">
          Live timing and strategy estimates. Click a driver to open telemetry.
        </caption>
        <thead>
          <tr>
            <th scope="col">Pos</th>
            {columns.map((c) => (
              <th scope="col" key={c}>
                {columnNames[c]}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {drivers.map((d) => (
            <tr key={`${d.driver.number}-${d.position}`}>
              <th scope="row">
                <span className="position">{d.position ?? "—"}</span>
                <small
                  className={
                    (d.start_position ?? 0) > (d.position ?? 0)
                      ? "personal-best"
                      : "muted"
                  }
                >
                  {d.start_position != null &&
                  d.position != null &&
                  d.start_position !== d.position
                    ? `${d.start_position > d.position ? "↑" : "↓"}${Math.abs(d.start_position - d.position)}`
                    : "·"}
                </small>
              </th>
              {columns.map((c) => (
                <td key={c}>{cell(c, d)}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
