use super::*;
use anyhow::Context;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, Transport};
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, Mutex};

#[derive(Clone)]
pub struct LiveFeed {
    pub events: broadcast::Sender<RaceEvent>,
    pub last_event: Arc<Mutex<Option<std::time::Instant>>>,
}
impl Default for LiveFeed {
    fn default() -> Self {
        Self::new()
    }
}
impl LiveFeed {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(4096);
        Self {
            events,
            last_event: Arc::new(Mutex::new(None)),
        }
    }
    pub async fn run(&self, session: u32) -> Result<()> {
        let username = std::env::var("OPENF1_USERNAME").ok();
        let password = std::env::var("OPENF1_PASSWORD").ok();
        loop {
            let token = if let (Some(user), Some(pass)) = (&username, &password) {
                let value: Value = reqwest::Client::new()
                    .post("https://api.openf1.org/token")
                    .form(&[("username", user), ("password", pass)])
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                value["access_token"]
                    .as_str()
                    .context("OpenF1 authentication did not issue a token")?
                    .to_string()
            } else {
                std::env::var("OPENF1_TOKEN")
                    .context("Live timing requires server-side OpenF1 sponsor credentials")?
            };
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
                            if row["session_key"].as_u64()!=Some(session as u64){continue;}
                            if let Some(event)=normalize(packet.topic.trim_start_matches("v1/"),&row,Some(chrono::Utc::now().timestamp_millis())) {
                                *self.last_event.lock().await=Some(std::time::Instant::now());let _=self.events.send(event);
                            }
                        },
                        Ok(_)=>{},Err(_)=>{tokio::time::sleep(Duration::from_secs(5)).await;break;}
                    }
                }
            }
        }
    }
}
