import type { Prediction } from "./types/Prediction";
import type { Snapshot } from "./types/Snapshot";
import { num } from "./view";

export default function RaceRisks({ prediction, drivers }: {
  prediction: Prediction | null | undefined;
  drivers: Snapshot["drivers"];
}) {
  const pass = prediction?.overtake;
  const forecasts = [
    { name: "New SC/VSC deployment", forecast: prediction?.safety_car },
    { name: pass ? `Pass ${drivers[pass.target]?.driver.code ?? `car ${pass.target}`}` : "Pass car ahead", forecast: pass?.forecast },
  ];
  return <section className="race-risks" aria-label="Race probability forecasts">
    {forecasts.map(({ name, forecast }) => <div key={name}>
      <strong>{name}</strong>
      <p>{forecast ? `${num(forecast.probability * 100, 1)}% · next ${forecast.horizon_laps} laps` : "Probability unavailable"}</p>
      <p className="source-note">{forecast?.label ?? "Requires a green race, five scheduled laps remaining, a supported circuit and observed model inputs."}</p>
      {forecast && <details>
        <summary>Why this probability?</summary>
        <ul>{forecast.factors.map((factor, i) => <li key={i}>{factor}</li>)}</ul>
        <p className="source-note">Experimental model · {forecast.model}</p>
      </details>}
    </div>)}
  </section>;
}
