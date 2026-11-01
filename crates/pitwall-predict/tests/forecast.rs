use pitwall_predict::*;
use pitwall_state::*;
use std::sync::Arc;
fn race() -> pitwall_core::Snapshot {
    let a = Arc::new(demo_archive());
    let start = a.session.start_ms;
    let mut r = Replay::new(a);
    r.seek(start + 480_000);
    r.state().snapshot()
}
#[test]
fn forecasts_have_ordered_widening_bands_and_finish_probabilities() {
    let s = race();
    let p = predict(&s);
    assert_eq!(p.len(), 20);
    let p = &p[0];
    assert_eq!(p.pace.len(), 5);
    for band in &p.pace {
        assert!(band.validate().is_ok());
    }
    assert!(p.pace[4].high - p.pace[4].low > p.pace[0].high - p.pace[0].low);
    assert!((p.finish.iter().map(|(_, v)| v).sum::<f64>() - 1.0).abs() < 0.001);
    assert!(p.model.contains("basic"));
    assert_eq!(p.factors.len(), 3);
}
#[test]
fn red_flags_pause_and_sc_lowers_confidence() {
    let mut s = race();
    let normal = predict(&s)[0].confidence;
    s.track_status = "SC".into();
    assert!(predict(&s)[0].confidence < normal);
    s.track_status = "RED".into();
    let p = predict(&s);
    assert_eq!(p[0].status, "paused");
    assert!(p[0].pace.is_empty());
}
#[test]
fn forecast_log_scores_only_completed_actuals_and_deduplicates() {
    let mut log = PredictionLog::default();
    let mut s = race();
    log.record(&s, &predict(&s));
    let count = log.records.len();
    log.record(&s, &predict(&s));
    assert_eq!(log.records.len(), count);
    assert_eq!(log.report().pace_samples, 0);
    let a = Arc::new(demo_archive());
    let start = a.session.start_ms;
    let mut r = Replay::new(a);
    r.seek(start + 560_000);
    s = r.state().snapshot();
    log.observe(&s);
    assert!(log.report().pace_samples > 0);
}
