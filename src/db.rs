use redis::{Client, RedisResult};
use tokio_stream::StreamExt;

pub async fn sub_key(channel: String) -> RedisResult<()> {
    let client = Client::open("redis://127.0.0.1/").unwrap();
    let conn = client.get_tokio_connection().await?;
    let mut pubsub = conn.into_pubsub();
    pubsub
        .subscribe(format!("__keyspace@0__:{}", channel))
        .await?;
    tokio::spawn(async move {
        while let Some(msg) = pubsub.on_message().next().await {
            println!("{:?}", msg);
        }
    });
    Ok(())
}
