use crate::{client, Clients, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::Reply;

#[derive(Deserialize, Debug)]
pub struct RegisterRequest {
    user_id: usize,
}

#[derive(Serialize, Debug)]
pub struct RegisterResponse {
    url: String,
}

#[derive(Deserialize, Debug)]
pub struct Event {
    topic: String,
    user_id: Option<usize>,
    message: String,
}

pub async fn ws_handler(ws: warp::ws::Ws, clients: Clients) -> Result<impl Reply> {
    let id = Uuid::new_v4().to_simple().to_string();
    let client = client::register_client(id.clone(), clients.clone()).await;
    Ok(ws.on_upgrade(move |socket| client::client_connection(socket, id, clients, client)))
}
