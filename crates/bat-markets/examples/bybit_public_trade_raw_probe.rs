use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::time::{Duration, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "wss://stream.bybit.com/v5/public/linear";
    let (mut ws, _) = connect_async(url).await?;
    let subscribe = json!({
        "op": "subscribe",
        "args": ["publicTrade.BTCUSDT", "orderbook.1.BTCUSDT"]
    })
    .to_string();
    ws.send(Message::Text(subscribe.into())).await?;

    for index in 0..10 {
        let Some(frame) = timeout(Duration::from_secs(10), ws.next()).await? else {
            println!("stream closed before receiving frame {}", index + 1);
            return Ok(());
        };
        let frame = frame?;
        println!("frame {}: {:?}", index + 1, frame);
        if let Message::Text(text) = &frame
            && text.contains("publicTrade.BTCUSDT")
        {
            break;
        }
    }

    Ok(())
}
