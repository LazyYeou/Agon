use super::*;
use anyhow::Context;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, Transport};
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, Mutex};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LiveArrival {
    pub session_key: u32,
    pub received_at_ms: i64,
    pub source_topic: String,
    pub source_key: String,
    pub source_id: i64,
    pub document: Value,
    pub event: RaceEvent,
}
impl LiveArrival {
    pub fn decode(topic: &str, row: &Value, session: u32, received_at_ms: i64) -> Option<Self> {
        if row["session_key"].as_u64()? != u64::from(session) {
            return None;
        }
        let source_key = row["_key"].as_str()?;
        let source_id = row["_id"].as_i64()?;
        if source_key.is_empty() || source_key.len() > 256 || source_id < 0 {
            return None;
        }
        let topic = topic.strip_prefix("v1/").unwrap_or(topic);
        Some(Self {
            session_key: session,
            received_at_ms,
            source_topic: topic.into(),
            source_key: source_key.into(),
            source_id,
            document: row.clone(),
            event: normalize(topic, row, Some(received_at_ms))?,
        })
    }
}

#[derive(Clone)]
pub struct LiveFeed {
    pub events: broadcast::Sender<RaceEvent>,
    pub last_event: Arc<Mutex<Option<std::time::Instant>>>,
    arrivals: mpsc::Sender<LiveArrival>,
    receiver: Arc<Mutex<Option<mpsc::Receiver<LiveArrival>>>>,
}

impl Default for LiveFeed {
    fn default() -> Self {
        Self::new()
    }
}
impl LiveFeed {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(4096);
        let (arrivals, receiver) = mpsc::channel(4096);
        Self {
            events,
            last_event: Arc::new(Mutex::new(None)),
            arrivals,
            receiver: Arc::new(Mutex::new(Some(receiver))),
        }
    }
    pub async fn take_arrivals(&self) -> Option<mpsc::Receiver<LiveArrival>> {
        self.receiver.lock().await.take()
    }
    pub async fn run(&self, session: u32) -> Result<()> {
        let source = OpenF1::new()?;
        loop {
            let token = source
                .access_token()
                .await?
                .context("Live timing requires server-side OpenF1 sponsor credentials")?;
            let mut options = MqttOptions::new(
                format!("agon-{}", std::process::id()),
                "mqtt.openf1.org",
                8883,
            );
            options.set_transport(Transport::tls_with_default_config());
            options.set_credentials("agon", token);
            options.set_keep_alive(Duration::from_secs(30));
            let (client, mut eventloop) = AsyncClient::new(options, 32);
            for topic in [
                "laps",
                "position",
                "intervals",
                "car_data",
                "location",
                "pit",
                "race_control",
                "weather",
                "stints",
            ] {
                client
                    .subscribe(format!("v1/{topic}"), QoS::AtMostOnce)
                    .await?;
            }
            let renew = tokio::time::sleep(Duration::from_secs(3500));
            tokio::pin!(renew);
            loop {
                tokio::select! {
                    _=&mut renew=>break,
                    result=eventloop.poll()=>match result {
                        Ok(Event::Incoming(Incoming::Publish(packet)))=>{
                            let Ok(row)=serde_json::from_slice::<Value>(&packet.payload)else{continue;};
                            if let Some(arrival)=LiveArrival::decode(&packet.topic,&row,session,chrono::Utc::now().timestamp_millis()) {
                                *self.last_event.lock().await=Some(std::time::Instant::now());
                                let _=self.events.send(arrival.event.clone());
                                if self.receiver.lock().await.is_none() {
                                    self.arrivals.send(arrival).await.context("Live arrival consumer stopped")?;
                                }
                            }
                        },
                        Ok(_)=>{},Err(_)=>{tokio::time::sleep(Duration::from_secs(5)).await;break;}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mqtt_tls_transport_builds_with_the_rest_client_crypto_features() {
        let _transport = Transport::tls_with_default_config();
    }
    #[test]
    fn live_arrival_preserves_document_revision_and_receipt_time() {
        let row = serde_json::json!({"session_key": 42, "_key": "position-1", "_id": 19,
            "driver_number": 1, "position": 3, "date": "2026-11-01T10:00:00Z"});
        let arrival = LiveArrival::decode("v1/position", &row, 42, 1234).unwrap();
        assert_eq!(arrival.source_key, "position-1");
        assert_eq!(arrival.source_id, 19);
        assert_eq!(arrival.received_at_ms, 1234);
        assert_eq!(arrival.document, row);
        assert!(LiveArrival::decode("v1/position", &row, 43, 1234).is_none());
        let mut invalid = row;
        invalid["_id"] = serde_json::json!(-1);
        assert!(LiveArrival::decode("v1/position", &invalid, 42, 1234).is_none());
    }
}
