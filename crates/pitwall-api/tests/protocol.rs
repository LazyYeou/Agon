use futures_util::{SinkExt, StreamExt};
use pitwall_core::*;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::test]
async fn websocket_clients_have_isolated_seek_and_validated_controls() {
    let app = pitwall_api::App::local(
        std::env::temp_dir().join(format!("agon-api-{}", std::process::id())),
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            pitwall_api::router(app).into_make_service_with_connect_info::<std::net::SocketAddr>(),
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
    let initial = b.next().await.unwrap().unwrap();
    let frame: ServerFrame = rmp_serde::from_slice(&initial.into_data()).unwrap();
    let ServerFrame::Snapshot { state: bs, .. } = frame else {
        panic!("snapshot expected")
    };
    assert_eq!(bs.clock_ms, state.clock_ms);
    a.send(Message::Text(r#"{"kind":"speed","speed":21}"#.into()))
        .await
        .unwrap();
    let error = a.next().await.unwrap().unwrap();
    assert!(matches!(
        rmp_serde::from_slice::<ServerFrame>(&error.into_data()).unwrap(),
        ServerFrame::Error { .. }
    ));
    let command = serde_json::to_string(&ClientCommand::Seek {
        time_ms: state.session.start_ms + 100_000,
    })
    .unwrap();
    a.send(Message::Text(command.into())).await.unwrap();
    let next = a.next().await.unwrap().unwrap();
    let frame: ServerFrame = rmp_serde::from_slice(&next.into_data()).unwrap();
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
    task.abort();
}
