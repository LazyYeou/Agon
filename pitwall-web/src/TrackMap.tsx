import { useState } from "react";
import type { Snapshot } from "./types/Snapshot";
import type { DriverState } from "./types/DriverState";
import { num } from "./view";
import { mapBounds } from "./analysis";
export default function TrackMap({
  state,
  onSelect,
  selected,
  miniPoint,
}: {
  state: Snapshot;
  onSelect: (n: number) => void;
  selected?: number | null;
  miniPoint?: [number, number] | null;
}) {
  const [labels, setLabels] = useState(true);
  const points = state.circuit.outline;
  const bounds = mapBounds(points);
  const poly = (pts: [number, number][]) =>
    pts.map(([x, y]) => `${x},${y}`).join(" ");
  const drivers = Object.values(state.drivers).filter(
    (d): d is DriverState => !!d,
  );
  const picked = selected ? state.drivers[selected] : null;
  const rejoin = picked?.prediction?.rejoin;
  const ghost = rejoin
    ? drivers.find((d) => d.position === Math.round(rejoin.value))?.location
    : null;
  return (
    <section className="map-panel">
      <div className="panel-heading">
        <h2>Track map</h2>
        <label className="inline-check">
          <input
            type="checkbox"
            checked={labels}
            onChange={(e) => setLabels(e.target.checked)}
          />
          Labels
        </label>
      </div>
      <div className="map-stage">
        {points.length > 2 ? (
          <svg
            viewBox={bounds.join(" ")}
            role="img"
            aria-label="Approximate circuit and current car positions"
          >
            <polyline
              points={poly(points)}
              fill="none"
              stroke="#29394d"
              strokeWidth="28"
              strokeLinejoin="round"
              strokeLinecap="round"
            />
            <polyline
              points={poly(points)}
              fill="none"
              stroke="#71859e"
              strokeWidth="3"
            />
            {state.circuit.sectors.map((fraction, i) => {
              const from = Math.floor(fraction * (points.length - 1));
              const to = Math.floor(
                (state.circuit.sectors[i + 1] ?? 1) * (points.length - 1),
              );
              return (
                <polyline
                  key={i}
                  points={poly(points.slice(from, to + 1))}
                  fill="none"
                  stroke={["#82b9f7", "#b19ae7", "#e8c17a"][i % 3]}
                  strokeWidth="4"
                />
              );
            })}
            {state.circuit.drs_zones.map(([a, b], i) => (
              <polyline
                key={`drs${i}`}
                points={poly(
                  points.slice(
                    Math.floor(a * points.length),
                    Math.floor(b * points.length),
                  ),
                )}
                fill="none"
                stroke="#69d6b6"
                strokeWidth="10"
                opacity="0.65"
              />
            ))}
            {state.circuit.pit_lane.length > 0 ? (
              <polyline
                points={poly(state.circuit.pit_lane)}
                fill="none"
                stroke="#8a9cb5"
                strokeWidth="3"
                strokeDasharray="8 7"
              />
            ) : null}
            {state.circuit.corners.map(([name, x, y]) => (
              <text
                key={name}
                x={x + 16}
                y={y - 14}
                fill="#9bacc0"
                fontSize="13"
              >
                {name}
              </text>
            ))}
            {drivers
              .filter((d) => d.location)
              .map((d) => (
                <g
                  key={d.driver.number}
                  role="button"
                  tabIndex={0}
                  aria-label={`Select ${d.driver.code} on map`}
                  onClick={() => onSelect(d.driver.number)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      onSelect(d.driver.number);
                    }
                  }}
                  className="car-dot"
                  transform={`translate(${d.location![0]},${d.location![1]})`}
                >
                  <title>
                    {d.driver.code} · P{d.position} · gap {num(d.gap)} s
                  </title>
                  <circle
                    r={selected === d.driver.number ? 12 : 8}
                    fill={`#${d.driver.color}`}
                    stroke="#0d1420"
                    strokeWidth="3"
                  />
                  {labels ? (
                    <text x="13" y="4" fill="#e4edf7" fontSize="12">
                      {d.driver.code}
                    </text>
                  ) : null}
                </g>
              ))}
            {ghost ? (
              <g transform={`translate(${ghost[0]},${ghost[1]})`}>
                <circle
                  r="17"
                  stroke="#e4edf7"
                  strokeWidth="2"
                  strokeDasharray="4 3"
                  fill="none"
                />
                <title>
                  {picked?.driver.code}: estimated rejoin P{num(rejoin?.low, 0)}
                  –P{num(rejoin?.high, 0)}
                </title>
              </g>
            ) : null}
            {miniPoint ? (
              <circle
                cx={miniPoint[0]}
                cy={miniPoint[1]}
                r="13"
                fill="none"
                stroke="#fff"
                strokeWidth="3"
              />
            ) : null}
          </svg>
        ) : (
          <div className="empty">
            <p>Position data unavailable</p>
            <small>
              A circuit outline appears when session location data is available.
            </small>
          </div>
        )}
        <span className="circuit-label">
          {state.session.circuit}
          <small>{state.session.country} · approximate position data</small>
        </span>
      </div>
      <div className="map-legend">
        <span>
          <i style={{ background: "#69d6b6" }} />
          DRS zone
        </span>
        <span>◌ Estimated rejoin group</span>
        <span>{state.circuit.corners.length} corner markers</span>
      </div>
      <p className="source-note">
        {state.circuit.source || "Waiting for source geometry"}
      </p>
      {!state.circuit.drs_zones.length || !state.circuit.pit_lane.length ? (
        <p className="source-note">
          DRS / pit geometry unavailable for this configuration.
        </p>
      ) : null}
    </section>
  );
}
