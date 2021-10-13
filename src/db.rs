use crate::client::{send_to_client, send_to_room, Client, Response};
use crate::Clients;
use redis::{Client as RedisClient, RedisResult};
use tokio_stream::StreamExt;

pub async fn global_sub_key(clients: Clients, channel: String) -> RedisResult<()> {
    let redis_cli = RedisClient::open("redis://127.0.0.1/").unwrap();
    let conn = redis_cli.get_tokio_connection().await?;
    let mut pubsub = conn.into_pubsub();
    pubsub
        .subscribe(format!("__keyspace@0__:{}", channel))
        .await?;
    tokio::spawn(async move {
        while let Some(msg) = pubsub.on_message().next().await {
            send_to_room(
                &clients,
                &None,
                &Response::Message {
                    message: msg.get_payload().unwrap(),
                },
            )
            .await;
        }
    });
    Ok(())
}

pub async fn client_sub_key(client: Client, channel: String) -> RedisResult<()> {
    let redis_cli = RedisClient::open("redis://127.0.0.1/").unwrap();
    let conn = redis_cli.get_tokio_connection().await?;
    let mut pubsub = conn.into_pubsub();
    pubsub
        .subscribe(format!("__keyspace@0__:{}", channel))
        .await?;
    tokio::spawn(async move {
        while let Some(msg) = pubsub.on_message().next().await {
            send_to_client(
                &client,
                &Response::Message {
                    message: msg.get_payload().unwrap(),
                },
            )
            .await;
        }
    });
    Ok(())
}
