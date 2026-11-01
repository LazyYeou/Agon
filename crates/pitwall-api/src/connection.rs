use super::*;
use axum::extract::ws::{Message, WebSocket};
use pitwall_predict::{predict, PredictionLog};
use std::sync::atomic::Ordering::Relaxed;

struct Active {
    app: Arc<App>,
    ip: IpAddr,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.app.connections.fetch_sub(1, Relaxed);
        if let Some(r) = self
            .app
            .rates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&self.ip)
        {
            r.2 = r.2.saturating_sub(1);
        }
    }
}
async fn send(socket: &mut WebSocket, app: &App, frame: &ServerFrame) -> bool {
    let Ok(bytes) = rmp_serde::to_vec_named(frame) else {
        return false;
    };
    app.sent.fetch_add(bytes.len() as u64, Relaxed);
    matches!(
        tokio::time::timeout(
            Duration::from_secs(3),
            socket.send(Message::Binary(bytes.into()))
        )
        .await,
        Ok(Ok(()))
    )
}
fn public(state: &Snapshot) -> Snapshot {
    let mut snapshot = state.clone();
    for d in snapshot.drivers.values_mut() {
        d.telemetry = None;
    }
    snapshot
}
pub fn diff(old: &Value, new: &Value) -> Option<Value> {
    if old == new {
        return None;
    }
    if let (Some(a), Some(b)) = (old.as_object(), new.as_object()) {
        let mut changed = serde_json::Map::new();
        for (key, value) in b {
            if let Some(patch) = diff(a.get(key).unwrap_or(&Value::Null), value) {
                changed.insert(key.clone(), patch);
            }
        }
        for key in a.keys() {
            if !b.contains_key(key) {
                changed.insert(key.clone(), Value::Null);
            }
        }
        Some(Value::Object(changed))
    } else {
        Some(new.clone())
    }
}
fn signature(s: &Snapshot) -> String {
    format!(
        "{}:{:?}:{}",
        s.track_status,
        s.weather,
        s.drivers
            .values()
            .map(|d| format!("{}:{}:{}", d.lap, d.stints.len(), d.pits.len()))
            .collect::<Vec<_>>()
            .join(",")
    )
}
pub async fn run(mut socket: WebSocket, app: Arc<App>, ip: IpAddr) {
    app.connections.fetch_add(1, Relaxed);
    let _active = Active {
        app: app.clone(),
        ip,
    };
    let Ok(archive) = app.archive(0).await else {
        return;
    };
    let mut replay = Replay::new(archive);
    replay.seek(replay.archive.session.start_ms + 400_000);
    let initial = predict(replay.state().view());
    for p in &initial {
        if let Some(d) = replay.state_mut().view_mut().drivers.get_mut(&p.driver) {
            d.prediction = Some(p.clone());
        }
    }
    let mut log = PredictionLog::default();
    log.record(replay.state().view(), &initial);
    let mut sequence = 1u32;
    let mut generation = 0u32;
    let mut subscribed = None;
    let mut playing = false;
    let mut speed = 1.0;
    let mut previous = serde_json::to_value(public(replay.state().view())).unwrap();
    if !send(
        &mut socket,
        &app,
        &ServerFrame::Snapshot {
            sequence,
            generation,
            state: Box::new(public(replay.state().view())),
        },
    )
    .await
    {
        return;
    }
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last = Instant::now();
    let mut last_signature = signature(replay.state().view());
    let mut job: Option<(u32, String, tokio::task::JoinHandle<Vec<Prediction>>)> = None;
    let mut command_window = Instant::now();
    let mut commands = 0;
    let mut run_id = format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    loop {
        tokio::select! {
            incoming=socket.recv()=>{
                let Some(Ok(message))=incoming else{break;};
                let Message::Text(text)=message else{if matches!(message,Message::Close(_)){break;}continue;};
                if command_window.elapsed()>Duration::from_secs(1){commands=0;command_window=Instant::now();}commands+=1;if commands>30{break;}
                let command=serde_json::from_str::<ClientCommand>(&text);let mut snapshot=false;let mut error=None;
                match command {
                    Ok(ClientCommand::Open {session})=>match app.archive(session).await {
                        Ok(a)=>{replay=Replay::new(a);generation=generation.wrapping_add(1);playing=false;subscribed=None;log=PredictionLog::default();run_id=format!("{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));snapshot=true;last_signature.clear();},
                        Err(_)=>error=Some("Session data unavailable. Retry later; live sessions require sponsor access."),
                    },
                    Ok(ClientCommand::Seek {time_ms})=>{
                        if time_ms<replay.archive.session.start_ms || time_ms>replay.archive.session.end_ms{error=Some("Seek outside session");}
                        else{replay.seek(time_ms);generation=generation.wrapping_add(1);log=PredictionLog::default();run_id=format!("{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));snapshot=true;last_signature.clear();}
                    },
                    Ok(ClientCommand::Play {playing:p})=>playing=p,
                    Ok(ClientCommand::Speed {speed:v}) if v.is_finite()&&(1.0..=20.0).contains(&v)=>speed=v,
                    Ok(ClientCommand::Subscribe {driver})=>{if driver.is_some_and(|n|!replay.state().view().drivers.contains_key(&n)){error=Some("Driver not in session");}else{subscribed=driver;}},
                    Ok(ClientCommand::Resync)=>snapshot=true,
                    _=>error=Some("Invalid timing command; speed must be between 1 and 20"),
                }
                if let Some(message)=error{if !send(&mut socket,&app,&ServerFrame::Error {message:message.into()}).await{break;}}
                replay.state_mut().view_mut().playing=playing;replay.state_mut().view_mut().speed=speed;
                if snapshot{if let Some((_,_,handle))=job.take(){handle.abort();}sequence=sequence.wrapping_add(1);previous=serde_json::to_value(public(replay.state().view())).unwrap();if !send(&mut socket,&app,&ServerFrame::Snapshot {sequence,generation,state:Box::new(public(replay.state().view()))}).await{break;}}
                last=Instant::now();
            },
            _=tick.tick()=>{
                let elapsed=last.elapsed();last=Instant::now();
                if playing{let next=replay.state().view().clock_ms+(elapsed.as_millis() as f64*speed) as i64;replay.advance(next);if next>=replay.archive.session.end_ms{playing=false;}}
                replay.state_mut().view_mut().playing=playing;replay.state_mut().view_mut().speed=speed;
                let current_signature=signature(replay.state().view());
                if job.as_ref().is_some_and(|(_,_,h)|h.is_finished()){
                    let (g,sig,handle)=job.take().unwrap();
                    if let Ok(predictions)=handle.await{if g==generation && sig==current_signature {
                        log.observe(replay.state().view());log.record(replay.state().view(),&predictions);
                        for p in predictions{if let Some(d)=replay.state_mut().view_mut().drivers.get_mut(&p.driver){d.prediction=Some(p);}}
                        if let Err(e)=app.store.log(&run_id,replay.archive.session.key,&log).await{tracing::error!(error=%e,"Forecast persistence failed");}
                    }}
                }
                let value=serde_json::to_value(public(replay.state().view())).unwrap();
                if let Some(changes)=diff(&previous,&value){sequence=sequence.wrapping_add(1);if !send(&mut socket,&app,&ServerFrame::Delta {sequence,generation,changes}).await{break;}previous=value;}
                if current_signature!=last_signature && job.is_none(){
                    if let Ok(permit)=app.jobs.clone().try_acquire_owned(){let state=replay.state().snapshot();job=Some((generation,current_signature.clone(),tokio::task::spawn_blocking(move||{let _permit=permit;predict(&state)})));last_signature=current_signature;}
                }
                if let Some(driver)=subscribed {let samples=replay.state().telemetry(driver);let recent=samples.into_iter().rev().take(16).collect::<Vec<_>>().into_iter().rev().collect();if !send(&mut socket,&app,&ServerFrame::Telemetry {driver,samples:recent}).await{break;}}
            }
        }
    }
    if let Some((_, _, h)) = job {
        h.abort();
    }
}
