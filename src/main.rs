use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::{Filter, Rejection};

mod client;
mod db;
mod handler;

type Result<T> = std::result::Result<T, Rejection>;
type Clients = Arc<RwLock<HashMap<uuid::Uuid, client::Client>>>;

#[tokio::main]
async fn main() {
    let clients: Clients = Arc::new(RwLock::new(HashMap::new()));

    match db::global_sub_key(clients.clone(), "test".to_string()).await {
        Ok(_) => println!("Subbed to changes on test key"),
        Err(e) => println!("{}", e),
    };

    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let rdb = redis::aio::ConnectionManager::new(client.clone()).await.unwrap();

    let static_files = warp::get().and(warp::fs::dir("www/static"));

    let ws_route = warp::path("api")
        .and(warp::ws())
        .and(warp::any().map(move || rdb.clone()))
        .and(warp::any().map(move || clients.clone()))
        .and_then(handler::ws_handler);

    let routes = static_files
        .or(ws_route)
        .with(warp::cors().allow_any_origin());

    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}
