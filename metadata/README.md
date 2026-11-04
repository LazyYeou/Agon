# Verified metadata

`verified.json` is the small bundled manifest. It currently covers only the scheduled
62 laps of Singapore 2023, OpenF1 session 9165. The source is Mercedes-AMG's
[pre-race article](https://www.mercedesamgf1.com/news/explained-the-singapore-grand-prix-circuit-changes),
published 12 September 2023, before the 17 September race. `known_at_ms` uses the
end of publication day UTC conservatively. Only the factual lap count and its
citation are recorded; article text, images and geometry are not redistributed,
and no licence for that content is claimed. OpenF1's session catalog independently
identifies the key, circuit and scheduled start. `coverage.json` distinguishes this
schedule coverage from the still-missing geometry and pit-loss coverage.

Set `AGON_METADATA_FILE` to an absolute path to a privately curated manifest to
replace the bundled manifest. Keep restricted source data and manifests under
`data/`, which is ignored by git. A missing configured file or an invalid manifest
is an error, rather than silently ignoring configuration. An uncovered session is
left unchanged. Historical loading, the session catalog, live setup and replay
conversion use the same `pitwall_ingest::metadata::apply_metadata` boundary.

Validate and optionally enrich a private `SessionArchive` JSON file:

```powershell
cargo run -p pitwall-ingest --example import_metadata -- data/metadata.json
cargo run -p pitwall-ingest --example import_metadata -- data/metadata.json data/race.json data/race-with-metadata.json
$env:AGON_METADATA_FILE = (Resolve-Path data/metadata.json).Path
```

The import command preserves existing output files. `MetadataManifest::from_path`
and `.apply(&mut session, &mut circuit)` are also available to offline callers;
manifests are validated before mutation, and a covered key with a conflicting
session identity is rejected. The manifest has `version: 1` and an `entries` array.
Unknown fields are rejected. Each entry requires the exact `session_key`, `year`,
`circuit` (OpenF1 short name), `session_name`, `start_ms`, and a human-readable
`configuration`. Add only verified components; omitted components are not inferred.

Every component has `provenance` with an HTTP(S) `source` URL, a nonempty `licence`
or permission/rights statement, and `known_at_ms` strictly before session start.
These are curator assertions: validation checks structure and dates, not the legal
validity of a claimed permission or the truth of a cited source. Inspect the source
and retain private evidence before import. A software repository's licence does not
automatically licence its upstream datasets.

- `schedule`: `total_laps` (1–500) plus provenance, only for Race/Sprint. It must
  come from a pre-race schedule. Completed lap maxima and final results are never
  schedule evidence. A shortened race can finish before its scheduled total.
- `geometry`: provenance, `outline` xy points, optional `corners` as `[name,x,y]`,
  `sectors` and `drs_zones` fractions, and `pit_lane` xy points; plus
  `coordinate_system: "openf1_location"`, `reference_session_key` equal to the
  target key, `alignment` describing the verified registration, `approximate`, and
  `fraction_basis: "outline_index"`. OpenF1 xy units are arbitrary. Register source
  geometry to the target session's coordinate frame before import; metres, GeoJSON
  longitude/latitude, and another session's unregistered coordinates are rejected.
  The outline starts at the timing line and follows lap direction. Fractions are
  **indices into this supplied outline**, matching the map contract, not metres or
  equal-distance fractions. Three timing sectors use `[0,s2_start,s3_start]`;
  marshal sectors are not timing sectors. DRS ranges are ordered `[start,end]`
  within `[0,1]`; split a zone crossing the timing line. All markers and pit points
  must lie near the outline bounds. No equal-spaced substitute markers are created.
- `pit_loss`: `band: {low,value,high}` in strategic lost-time seconds (not pit-lane
  duration), `calibration_sessions` listing `{session_key,end_ms}` records, `data_through_ms`,
  `method`, and provenance. The target race cannot calibrate its own prediction;
  each calibration session must end by the data cutoff, which must be at or before
  source availability and before target start. Curators
  must verify every listed calibration session precedes the target and retain their
  method/data privately. Intervals must be ordered, positive and at most 300 seconds.

For a minimal real schema example, copy `verified.json` into `data/metadata.json`.
The unit tests in `crates/pitwall-ingest/src/metadata.rs` additionally demonstrate
synthetic overlays and pit-loss calibration; those coordinates and values are test
fixtures, not reusable racing data. Applying schedule-only metadata preserves the
existing approximate OpenF1 outline. Complete sourced coverage since 2023 remains
an external curation/licensing requirement.
