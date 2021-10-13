use crate::Clients;
use futures::{FutureExt, StreamExt};
use redis::Commands;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use warp::ws::{Message, WebSocket};
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "message")]
    Chat {
        room: Option<String>,
        message: String,
    },
    #[serde(rename = "login")]
    Login { username: String, password: String },
    #[serde(rename = "register")]
    Register { username: String, password: String },
    #[serde(rename = "loginWithID")]
    LoginWithID { id: String },
}
#[derive(Serialize, Debug)]
#[serde(tag = "type")]
pub enum Response {
    #[serde(rename = "login")]
    Login {
        user: Option<User>,
        message: Option<String>,
        status: Option<String>,
        #[serde(rename = "sessionID")]
        session_id: Option<String>,
    },
    #[serde(rename = "message")]
    Message { message: String },
}
#[derive(Debug, Clone)]
pub struct Client {
    pub id: usize,
    pub topics: Vec<String>,
    pub sender: Option<mpsc::UnboundedSender<std::result::Result<Message, warp::Error>>>,
}

#[derive(Serialize, Debug)]
pub struct User {
    pub username: String,
}

pub async fn client_connection(ws: WebSocket, id: String, clients: Clients, mut client: Client) {
    let (client_ws_sender, mut client_ws_rcv) = ws.split();
    let (client_sender, client_rcv) = mpsc::unbounded_channel();
    let client_rcv = UnboundedReceiverStream::new(client_rcv); // <-- this

    tokio::task::spawn(client_rcv.forward(client_ws_sender).map(|result| {
        if let Err(e) = result {
            eprintln!("error sending websocket msg: {}", e);
        }
    }));

    client.sender = Some(client_sender);
    clients.write().await.insert(id.clone(), client);

    println!("{} connected", id);

    while let Some(result) = client_ws_rcv.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("error receiving ws message for id: {}): {}", id.clone(), e);
                break;
            }
        };
        message_handler(&id, msg, &clients).await;
    }

    unregister_client(&id, clients).await;
    println!("{} disconnected", id);
}

///handles incoming messages for a client
async fn message_handler(id: &str, msg: Message, clients: &Clients) {
    let message = match msg.to_str() {
        Ok(v) => v,
        Err(_) => return,
    };

    let request: Request = match from_str(&message) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error parsing request: {}", e);
            return;
        }
    };
    match &request {
        Request::Chat { message, room } => {
            send_to_room(
                clients,
                room,
                &Response::Message {
                    message: message.clone(),
                },
            )
            .await;
        }
        Request::Login { username, password } => {
            let client = clients.clone().read().await.get(id).unwrap().clone();
            // let thing = local.get(id).unwrap(); //.read().await.get(id).unwrap();
            let rdb = redis::Client::open("redis://127.0.0.1/").unwrap();
            let mut conn = rdb.get_connection().unwrap();

            let data: HashMap<String, String> = conn
                .hgetall("user:".to_string() + &username.clone())
                .unwrap();

            if data.is_empty() {
                send_to_client(
                    &client,
                    &Response::Login {
                        message: Some("User does not exist".to_string()),
                        status: None,
                        user: None,
                        session_id: None,
                    },
                )
                .await;
                return;
            }
            if data.get("password").unwrap() != password {
                send_to_client(
                    &client,
                    &Response::Login {
                        message: Some("Incorrect password".to_string()),
                        status: None,
                        user: None,
                        session_id: None,
                    },
                )
                .await;
                return;
            }
            let session_id = uuid::Uuid::new_v4().to_string();
            let key = "session:".to_string() + &session_id;
            let _: () = conn.hset(&key, "username", &username).unwrap();
            let _: () = conn.expire(&key, 6).unwrap();
            send_to_client(
                &client,
                &Response::Login {
                    user: Some(User {
                        username: username.to_string(),
                    }),
                    status: Some("success".to_string()),
                    message: None,
                    session_id: Some(session_id),
                },
            )
            .await;
        }
        Request::Register { username, password } => {
            //register
        }
        Request::LoginWithID { id } => {
            //login with id
        }
    }
}

///regesters a client to the Client pool
pub async fn register_client(id: String, clients: Clients) -> Client {
    let client = Client {
        id: 0,
        topics: vec![String::from("cats")],
        sender: None,
    };
    clients.write().await.insert(id, client.clone());
    return client;
}

///unregesters a client from the Client pool
pub async fn unregister_client(id: &String, clients: Clients) {
    clients.write().await.remove(id);
}

///sends a response to one client
pub async fn send_to_client(client: &Client, response: &Response) {
    if let Some(sender) = &client.sender {
        let _ = sender.send(Ok(Message::text(to_string(&response).unwrap())));
    }
}

///sends a response to all clients in a room. If room is left empty, sends to all clients
pub async fn send_to_room(clients: &Clients, room: &Option<String>, response: &Response) {
    let response = to_string(&response).unwrap();
    clients
        .read()
        .await
        .iter()
        .filter(|(_, client)| match &room {
            Some(v) => client.topics.contains(&v),
            None => true,
        })
        .for_each(|(_, client)| {
            if let Some(sender) = &client.sender {
                let _ = sender.send(Ok(Message::text(&response)));
            }
        });
}
