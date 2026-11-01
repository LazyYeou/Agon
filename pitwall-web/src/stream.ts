import { useEffect, useRef, useState, useCallback } from "react";
import { decode } from "@msgpack/msgpack";
import type { Snapshot } from "./types/Snapshot";
import type { ServerFrame } from "./types/ServerFrame";
import type { ClientCommand } from "./types/ClientCommand";
import type { TelemetrySample } from "./types/TelemetrySample";
import type { PaceForecast } from "./types/PaceForecast";
import type { AccuracyReport } from "./types/AccuracyReport";
import { mergeDelta, readView } from "./view";
import { mergeForecasts } from "./analysis";
export function useTiming() {
  const [state, setState] = useState<Snapshot | null>(null);
  const [status, setStatus] = useState("connecting");
  const [error, setError] = useState("");
  const [samples, setSamples] = useState<{
    driver: number;
    samples: TelemetrySample[];
  } | null>(null);
  const [evaluation, setEvaluation] = useState<{
    forecasts: PaceForecast[];
    report: AccuracyReport;
  } | null>(null);
  const socket = useRef<WebSocket | null>(null);
  const latest = useRef<Snapshot | null>(null);
  const subscription = useRef<number | null>(null);
  useEffect(() => {
    let cancelled = false;
    let retry: ReturnType<typeof setTimeout>;
    let attempts = 0;
    let sequence = 0;
    let generation = -1;
    function connect() {
      if (cancelled) return;
      setStatus("connecting");
      const ws = new WebSocket(
        `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`,
      );
      ws.binaryType = "arraybuffer";
      socket.current = ws;
      ws.onopen = () => {
        if (cancelled) {
          ws.close();
          return;
        }
        setStatus("connected");
        attempts = 0;
        sequence = 0;
        generation = -1;
        const remembered = latest.current;
        const shared = readView(location.search);
        if (
          remembered?.data_status === "live" ||
          remembered?.data_status === "data delayed"
        ) {
          ws.send(JSON.stringify({ kind: "live" }));
        } else if (remembered) {
          ws.send(
            JSON.stringify({ kind: "open", session: remembered.session.key }),
          );
          ws.send(
            JSON.stringify({ kind: "seek", time_ms: remembered.clock_ms }),
          );
          ws.send(JSON.stringify({ kind: "speed", speed: remembered.speed }));
          ws.send(
            JSON.stringify({ kind: "play", playing: remembered.playing }),
          );
        } else if (shared.session !== 0)
          ws.send(JSON.stringify({ kind: "open", session: shared.session }));
        if (subscription.current)
          ws.send(
            JSON.stringify({ kind: "subscribe", driver: subscription.current }),
          );
      };
      ws.onmessage = (event) => {
        if (cancelled) return;
        try {
          const frame = decode(new Uint8Array(event.data)) as ServerFrame;
          if (frame.kind === "error") {
            setError(frame.message);
            return;
          }
          if (frame.kind === "telemetry") {
            setSamples({ driver: frame.driver, samples: frame.samples });
            return;
          }
          if (frame.kind === "evaluation") {
            if (frame.generation === generation)
              setEvaluation(previous => ({
                forecasts: mergeForecasts(previous?.forecasts ?? [], frame.forecasts),
                report: frame.report,
              }));
            return;
          }
          if (frame.kind === "snapshot") {
            if (generation !== frame.generation) { setEvaluation(null); setSamples(null); }
            generation = frame.generation;
            sequence = frame.sequence;
            latest.current = frame.state;
            setState(frame.state);
            setError("");
          } else if (
            frame.generation === generation &&
            frame.sequence === sequence + 1
          ) {
            sequence = frame.sequence;
            setState((previous) => {
              if (!previous) return previous;
              const next = mergeDelta(previous, frame.changes);
              latest.current = next;
              return next;
            });
          } else {
            ws.send(JSON.stringify({ kind: "resync" }));
          }
        } catch {
          setError("Timing update could not be read. Resynchronising.");
          ws.send(JSON.stringify({ kind: "resync" }));
        }
      };
      ws.onclose = () => {
        if (cancelled) return;
        setStatus("data delayed");
        retry = setTimeout(connect, Math.min(10000, 500 * 2 ** attempts++));
      };
      ws.onerror = () => {
        setStatus("data delayed");
      };
    }
    connect();
    return () => {
      cancelled = true;
      clearTimeout(retry);
      socket.current?.close();
    };
  }, []);
  const send = useCallback((command: ClientCommand) => {
    if (command.kind === "subscribe") {
      subscription.current = command.driver;
      setSamples(null);
    }
    if (socket.current?.readyState === WebSocket.OPEN) {
      setError("");
      socket.current.send(JSON.stringify(command));
    } else setError("Connection interrupted. Reconnecting automatically.");
  }, []);
  return { state, status, error, setError, samples, send, evaluation };
}
