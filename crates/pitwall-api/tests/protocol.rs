use futures_util::{SinkExt, StreamExt};
use pitwall_core::*;
use tokio_tungstenite::{connect_async, tungstenite::Message};
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn next_matching(socket: &mut Socket, predicate: fn(&ServerFrame) -> bool) -> ServerFrame {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if !matches!(message, Message::Binary(_)) {
                continue;
            }
            let frame = rmp_serde::from_slice::<ServerFrame>(&message.into_data()).unwrap();
            if predicate(&frame) {
                return frame;
            }
        }
    })
    .await
    .expect("expected protocol frame within five seconds")
}

#[tokio::test]
async fn websocket_clients_have_isolated_seek_and_validated_controls() {
    let app = pitwall_api::App::local(
        std::env::temp_dir().join(format!("agon-api-{}", std::process::id())),
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            pitwall_api::router(server_app)
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let (mut a, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let (mut b, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let first = a.next().await.unwrap().unwrap();
    let frame: ServerFrame = rmp_serde::from_slice(&first.into_data()).unwrap();
    let ServerFrame::Snapshot { state, .. } = frame else {
        panic!("snapshot expected")
    };
    assert_eq!(state.drivers.len(), 20);
    let evaluation = next_matching(&mut a, |f| matches!(f, ServerFrame::Evaluation { .. })).await;
    assert!(matches!(evaluation,ServerFrame::Evaluation {forecasts,..} if !forecasts.is_empty()));
    let initial = b.next().await.unwrap().unwrap();
    let frame: ServerFrame = rmp_serde::from_slice(&initial.into_data()).unwrap();
    let ServerFrame::Snapshot { state: bs, .. } = frame else {
        panic!("snapshot expected")
    };
    assert_eq!(bs.clock_ms, state.clock_ms);
    a.send(Message::Text(r#"{"kind":"speed","speed":21}"#.into()))
        .await
        .unwrap();
    next_matching(&mut a, |f| matches!(f, ServerFrame::Error { .. })).await;
    let command = serde_json::to_string(&ClientCommand::Seek {
        time_ms: state.session.start_ms + 100_000,
    })
    .unwrap();
    a.send(Message::Text(command.into())).await.unwrap();
    let frame = next_matching(&mut a, |f| matches!(f, ServerFrame::Snapshot { .. })).await;
    let ServerFrame::Snapshot {
        state: after,
        generation,
        ..
    } = frame
    else {
        panic!("snapshot expected after seek")
    };
    assert_eq!(generation, 1);
    assert!(after.lap < bs.lap);
    let response = reqwest::get(format!("http://{address}/api/sessions?year=2022"))
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    a.send(Message::Text(r#"{"kind":"live"}"#.into()))
        .await
        .unwrap();
    next_matching(&mut a, |f| matches!(f, ServerFrame::Error { .. })).await;
    a.send(Message::Text(r#"{"kind":"play","playing":true}"#.into()))
        .await
        .unwrap();
    let delta = next_matching(
        &mut a,
        |f| matches!(f,ServerFrame::Delta {changes,..} if changes["playing"]==true),
    )
    .await;
    assert!(matches!(delta, ServerFrame::Delta { generation: 1, .. }));
    let runtime = std::sync::Arc::new(pitwall_api::LiveRuntime::new(std::sync::Arc::new(
        pitwall_state::demo_archive(),
    )));
    *app.live.write().await = Some(runtime);
    a.send(Message::Text(r#"{"kind":"live"}"#.into()))
        .await
        .unwrap();
    next_matching(&mut a, |f| {
        matches!(f, ServerFrame::Snapshot { generation: 2, .. })
    })
    .await;
    let evaluation = next_matching(&mut a, |f| {
        matches!(f, ServerFrame::Evaluation { generation: 2, .. })
    })
    .await;
    assert!(
        matches!(evaluation, ServerFrame::Evaluation {forecasts,report,..} if forecasts.is_empty() && report.pace_samples==0)
    );
    task.abort();
}
