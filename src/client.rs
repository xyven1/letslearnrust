use futures::{FutureExt, StreamExt};
use redis::{aio::ConnectionManager, AsyncCommands};
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;
use warp::ws::{Message, WebSocket};

use crate::Clients;
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum Request<'a> {
    #[serde(rename = "message")]
    Chat {
        room: Option<String>,
        message: &'a str,
    },
    #[serde(rename = "login")]
    Login {
        username: &'a str,
        password: &'a str,
    },
    #[serde(rename = "loginWithID")]
    LoginWithID {
        #[serde(rename = "sessionID")]
        session_id: &'a str,
    },
    #[serde(rename = "register")]
    Register {
        username: &'a str,
        password: &'a str,
    },
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
    #[serde(rename = "loginWithID")]
    LoginWithID {
        user: Option<User>,
        message: Option<String>,
    },
    #[serde(rename = "register")]
    Register {
        user: Option<User>,
        message: Option<String>,
        #[serde(rename = "sessionID")]
        session_id: Option<String>,
    },
    #[serde(rename = "message")]
    Message { message: String },
}
#[derive(Debug, Clone)]
pub struct Client {
    pub id: uuid::Uuid,
    pub topics: HashSet<String>,
    pub sender: Option<mpsc::UnboundedSender<std::result::Result<Message, warp::Error>>>,
}

impl Client {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            topics: HashSet::new(),
            sender: None,
        }
    }
    pub fn add_topic(&mut self, topic: String) {
        self.topics.insert(topic);
    }
    pub fn remove_topic(&mut self, topic: String) {
        self.topics.remove(&topic);
    }
}

#[derive(Serialize, Debug)]
pub struct User {
    pub username: String,
}

pub async fn client_connection(ws: WebSocket, rdb: ConnectionManager, clients: Clients) {
    let (client_ws_sender, mut client_ws_rcv) = ws.split();
    let (client_sender, client_rcv) = mpsc::unbounded_channel();
    let client_rcv = UnboundedReceiverStream::new(client_rcv);
    let client = Client::new();
    let id = {
        let mut cs = clients.write().await;
        let c = cs.entry(client.id).or_insert(client);
        c.sender = Some(client_sender);
        c.id
    };
    println!("{} connected", id);
    tokio::select!(
        _ = client_rcv.forward(client_ws_sender).map(|result| {
            if let Err(e) = result {
                eprintln!("error sending websocket msg: {}", e);
            }
        }) => {}
        _ = async {
            while let Some(result) = client_ws_rcv.next().await {
                let msg = match result {
                    Ok(msg) => msg,
                    Err(e) => {
                        eprintln!("error receiving ws message for id: {}): {}", id, e);
                        break;
                    }
                };
                message_handler(rdb.clone(), id, msg, clients.clone()).await;
            }
        } => {}
    );
    clients.write().await.remove(&id);
    println!("{} disconnected", id);
}

///handles incoming messages for a client
async fn message_handler(
    mut rdb: ConnectionManager,
    id: uuid::Uuid,
    msg: Message,
    clients: Clients,
) {
    let message = match msg.to_str() {
        Ok(v) => v,
        Err(_) => return,
    };

    let request = match from_str(message) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error parsing request: {}", e);
            return;
        }
    };
    match request {
        Request::Chat { message, room } => {
            send_to_room(
                clients.clone(),
                room,
                &Response::Message {
                    message: message.to_owned(),
                },
            )
            .await;
        }
        Request::Login { username, password } => {
            let client = match clients.read().await.get(&id) {
                Some(c) => c.clone(),
                None => return,
            };
            let data: HashMap<String, String> =
                rdb.hgetall("user:".to_string() + username).await.unwrap();

            if data.is_empty() {
                send_to_client(
                    client,
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
                    client,
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
            let session_id = get_session(&mut rdb, username).await;
            send_to_client(
                client,
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
            let client = match clients.read().await.get(&id) {
                Some(c) => c.clone(),
                None => return,
            };
            if let Err(e) = validate_username(username) {
                send_to_client(
                    client,
                    &Response::Register {
                        user: None,
                        message: Some(e),
                        session_id: None,
                    },
                )
                .await;
                return;
            }
            if let Err(e) = validate_password(password) {
                send_to_client(
                    client,
                    &Response::Register {
                        user: None,
                        message: Some(e),
                        session_id: None,
                    },
                )
                .await;
                return;
            }
            let data: HashMap<String, String> =
                rdb.hgetall("user:".to_string() + username).await.unwrap();
            if !data.is_empty() {
                send_to_client(
                    client,
                    &Response::Register {
                        user: None,
                        message: Some("User already exists".to_string()),
                        session_id: None,
                    },
                )
                .await;
                return;
            }
            let _: () = rdb
                .hset_multiple(
                    "user:".to_string() + username,
                    &[("username", username), ("password", password)],
                )
                .await
                .unwrap();
            let session_id = get_session(&mut rdb, username).await;
            send_to_client(
                client,
                &Response::Register {
                    user: Some(User {
                        username: username.to_string(),
                    }),
                    message: None,
                    session_id: Some(session_id),
                },
            )
            .await;
        }
        Request::LoginWithID { session_id } => {
            let client = match clients.read().await.get(&id) {
                Some(c) => c.clone(),
                None => return,
            };
            match rdb
                .hget("session:".to_string() + session_id, "username")
                .await
            {
                Ok(v) => {
                    send_to_client(
                        client,
                        &Response::LoginWithID {
                            user: Some(User { username: v }),
                            message: None,
                        },
                    )
                    .await;
                }
                Err(_) => {
                    send_to_client(
                        client,
                        &Response::LoginWithID {
                            user: None,
                            message: Some("Session is invalid".to_string()),
                        },
                    )
                    .await;
                }
            };
        }
    }
}

async fn get_session(rdb: &mut ConnectionManager, username: &str) -> String {
    let session_id = uuid::Uuid::new_v4().to_string();
    let key = "session:".to_string() + &session_id;
    let _: () = rdb.hset(&key, "username", username).await.unwrap();
    let _: () = rdb.expire(&key, 60).await.unwrap();
    session_id
}

fn validate_username(username: &str) -> Result<(), String> {
    match username.len() {
        0..=2 => Err("Username must be at least 3 characters".to_string()),
        3..=20 => Ok(()),
        _ => Err("Username cannot be longer than 20 characters".to_string()),
    }?;
    if !username.chars().next().unwrap().is_alphabetic() {
        return Err("Username must begin with a letter".to_string());
    }
    for c in username.chars() {
        if !c.is_alphanumeric() && c != '_' && c != '-' && c != '.' {
            return Err("Username must contain only letters, numbers, _, -, and .".to_string());
        }
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), String> {
    match password.len() {
        0 => Err("Password cannot be empty".to_string()),
        1..=7 => Err("Password must be at least 8 characters".to_string()),
        8..=120 => Ok(()),
        _ => Err("Password cannot be longer than 120 characters".to_string()),
    }?;
    Ok(())
}

///sends a response to one client
pub async fn send_to_client(client: Client, response: &Response) {
    if let Some(sender) = client.sender {
        let _ = sender.send(Ok(Message::text(to_string(response).unwrap())));
    }
}

///sends a response to all clients in a room. If room is left empty, sends to all clients
pub async fn send_to_room(clients: Clients, room: Option<String>, response: &Response) {
    let response = match to_string(response) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error serializing response: {}", e);
            return;
        }
    };
    clients
        .read()
        .await
        .iter()
        .filter(|(_, client)| match &room {
            Some(v) => client.topics.contains(v),
            None => true,
        })
        .for_each(|(_, client)| {
            if let Some(sender) = &client.sender {
                let _ = sender.send(Ok(Message::text(&response)));
            }
        });
}
