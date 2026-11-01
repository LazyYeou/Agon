# Source contracts verified 2026-11-01

Official references: <https://openf1.org/docs/>, <https://openf1.org/auth.html>, <https://openf1.org/account>.

- REST base `https://api.openf1.org/v1/`; free limit 3 requests/s **and 30/min**. The shared client uses at least 2.1 s between requests and caches completed historical queries.
- Token endpoint `POST https://api.openf1.org/token` accepts form username/password. MQTT TLS `mqtt.openf1.org:8883`, arbitrary nonempty username and token password; topics `v1/laps`, `v1/location`, etc. WSS is MQTT, not JSON WebSocket. Token expiry requires reauthentication/reconnect. Secrets remain server-side.
- `_key` identifies live document revisions; `_id` is delivery order. Laps are upserts. REST has no documented historical revision/arrival-time stream or reliable streaming resume cursor.
- `laps.date_start` is approximate start, not completion. Publish completed fields no earlier than start + duration. Null durations remain missing. Historical replay cannot reproduce original receipt times exactly.
- `stints` has no timestamp; map start to lap start and withhold future `lap_end`. `position` and `session_result` are singular endpoint names.
- `intervals` is race-only; gaps can be floats, null, or lap strings. Numeric gap projections are unavailable for lap strings. `car_data.drs` 10/12/14 on, 0/1 off; other values remain unknown.
- `pit.lane_duration` replaces deprecated `pit_duration`. `stop_duration` only exists from 2024 US GP. Lane duration is not strategic pit loss.
- `location` coordinates have arbitrary units; integrate speed over sample time for distance, never convert raw xy units into metres.
- Lap mini-sector status arrays are **unavailable in races**, and contain status codes, not sector duration/geometry. Derived telemetry comparison bins must be labelled approximate.
- Race-control `sector` is not reliably one of three timing sectors. Session catalog does not supply scheduled race lap totals. Final results are post-session data and cannot be prediction inputs.

## Circuit coverage

The map can derive its outline from the selected session's location data. This is approximate. No equally spaced markers are substituted for real corners/sectors/DRS/pit lane.

OpenF1 meetings expose `circuit_info_url` pointing to MultiViewer: <https://api.multiviewer.app/api/v1/circuits/61/2023>. Inspected Singapore data supplies corners, marshal sectors, outline, and pit-loss estimates, but no pit-lane polyline, DRS zones, or three timing-sector boundaries. Raw `length` is not metres. <https://docs.fastf1.dev/api_reference/circuit_info.html> describes visualization-grade markers fitted against reference telemetry.

No explicit MultiViewer dataset redistribution license was found. It is therefore not bundled under an invented license. <https://github.com/bacinger/f1-circuits> supplies MIT GeoJSON outlines but lacks the other metadata and per-year configuration coverage. Software licenses do not establish upstream data licenses.

`metadata/coverage.json` records the current verified coverage. Complete cited metadata for all sessions since 2023 remains an acceptance gap until acquired/curated. The synthetic fixture's metadata is only valid for its invented test circuit.

OpenF1 repository license: <https://github.com/br-g/openf1/blob/main/LICENSE> (CC BY-NC-SA 4.0). App scope remains non-commercial and attributed; raw datasets are stored privately for processing, not exposed as a raw proxy/download API.
