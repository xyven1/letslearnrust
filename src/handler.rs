use crate::{client, Clients, Result};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use warp::Reply;

#[derive(Deserialize, Debug)]
pub struct RegisterRequest {
    user_id: usize,
}

#[derive(Serialize, Debug)]
pub struct RegisterResponse {
    url: String,
}

pub async fn ws_handler(ws: warp::ws::Ws, rdb: ConnectionManager, clients: Clients) -> Result<impl Reply> {
    Ok(ws.on_upgrade(move |socket| client::client_connection(socket, rdb, clients)))
}
