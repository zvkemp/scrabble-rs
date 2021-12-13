// use async_session::{CookieStore, SessionStore};
use axum::{
    async_trait,
    extract::{Extension, FromRequest, RequestParts},
    http::{self, StatusCode},
};
use axum_channels::{
    channel::{self, Channel, NewChannel, Presence},
    message::{DecoratedMessage, Message, MessageKind},
    registry::Registry,
    types::{ChannelId, Token},
};
use scrabble::{Game, Player};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tracing::{debug, error};
use users::User;

use crate::session::Session;

// mod auth;
mod scrabble;
mod session;
mod users;
mod web;

// TODO list:
// - async-store for session cookies

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect("postgres://localhost/scrabble_rs")
        .await
        .unwrap();

    // FIXME: internalize the Arc/Mutex
    let registry = Arc::new(Mutex::new(Registry::default()));
    let mut locked = registry.lock().unwrap();
    let game_channel = GameChannel::new(pool.clone());
    locked.register_template("game".to_string(), game_channel);

    drop(locked);

    // let store = CookieStore::new();

    let app = web::app(registry, pool);

    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

struct PlayerIndex(usize);

#[derive(Debug)]
struct GameChannel {
    pub(crate) game: Game,
    pub(crate) socket_state: HashMap<Token, http::Extensions>,
    pub(crate) pg_pool: PgPool,
}

impl GameChannel {
    pub fn new(pg_pool: PgPool) -> Self {
        let game = Game::default();

        GameChannel {
            game,
            socket_state: HashMap::new(),
            pg_pool,
        }
    }

    fn play(&mut self, payload: serde_json::Value) -> Result<(), scrabble::Error> {
        let turn = payload.try_into()?;
        self.game.play(turn)?;

        Ok(())
    }
}

#[async_trait]
impl Channel for GameChannel {
    fn handle_message(&mut self, message: &DecoratedMessage) -> Option<Message> {
        match &message.inner.kind {
            MessageKind::Event => match message.inner.event.as_str() {
                "start" => {
                    let _ = self.game.start();

                    Some(Message {
                        kind: MessageKind::BroadcastIntercept,
                        channel_id: message.channel_id().clone(),
                        channel_sender: None,
                        join_ref: None,
                        msg_ref: message.msg_ref.clone(),
                        event: "player-state".into(),
                        payload: serde_json::Value::Null,
                    })
                }

                "play" => {
                    // FIXME: ensure play comes from current player
                    match self.play(message.inner.payload.clone()) {
                        Ok(_) => Some(Message {
                            kind: MessageKind::BroadcastIntercept,
                            channel_id: message.channel_id().clone(),
                            channel_sender: None,
                            join_ref: None,
                            msg_ref: message.msg_ref.clone(),
                            event: "player-state".into(),
                            payload: json!(null),
                        }),
                        Err(e) => {
                            error!("{:?}", e);

                            None
                        }
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn handle_out(&mut self, message: &DecoratedMessage) -> Option<Message> {
        match &message.inner.kind {
            MessageKind::BroadcastIntercept => {
                let index = self
                    .socket_state
                    .get(&message.token)
                    .unwrap()
                    .get::<PlayerIndex>()
                    .unwrap();

                match message.inner.event.as_str() {
                    "player-state" => {
                        let payload = self.game.player_state(index.0);
                        let reply = Message {
                            kind: MessageKind::Push,
                            channel_sender: None,
                            join_ref: None,
                            msg_ref: message.msg_ref.clone(),
                            channel_id: message.channel_id().clone(),
                            event: message.inner.event.clone(),
                            payload,
                        };

                        Some(reply)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    async fn handle_join(
        &mut self,
        message: &DecoratedMessage,
    ) -> Result<Option<Message>, channel::Error> {
        debug!("{:?}", message);
        let token = message
            .inner
            .payload
            .get("token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| channel::Error::Other("token not found".into()))
            .and_then(|token| Ok(Session::read_token(token.to_string())))?;

        let session = token.ok_or_else(|| channel::Error::Other("token was not valid".into()))?;

        let user = User::find(session.user_id.unwrap(), &self.pg_pool)
            .await // damn it
            .unwrap(); // FIXME: unwrap

        let player = Player(dbg!(user).username);

        let player_index = self
            .game
            .add_player(player)
            .map_err(|e| channel::Error::Join {
                reason: format!("{:?}", e),
            })?;

        let state = self.socket_state.entry(message.token).or_default();

        state.insert(PlayerIndex(player_index));

        Ok(Some(Message {
            kind: MessageKind::BroadcastIntercept,
            channel_id: message.channel_id().clone(),
            channel_sender: None,
            join_ref: None,
            msg_ref: message.msg_ref.clone(),
            event: "player-state".into(),
            payload: serde_json::Value::Null,
        }))
    }

    fn handle_leave(
        &mut self,
        message: &DecoratedMessage,
    ) -> axum_channels::channel::Result<Option<Message>> {
        self.socket_state.remove(&message.token);
        Ok(None)
    }

    fn handle_presence(
        &mut self,
        channel_id: &ChannelId,
        presence: &Presence,
    ) -> axum_channels::channel::Result<Option<Message>> {
        let mut online = HashSet::new();

        dbg!(presence);

        for user in presence.data.values() {
            online.insert(user.get("player").unwrap().as_str().unwrap());
        }

        let message = Message {
            kind: MessageKind::Broadcast,
            channel_id: channel_id.clone(),
            msg_ref: None,
            join_ref: None,
            payload: serde_json::json!({ "online": online.iter().collect::<Vec<_>>() }),
            event: "presence".into(),
            channel_sender: None,
        };

        Ok(Some(message))
    }
}

impl NewChannel for GameChannel {
    fn new_channel(&self) -> Box<dyn Channel> {
        Box::new(GameChannel::new(self.pg_pool.clone()))
    }
}

// trait Partial: Template + Display {}

// impl Partial for GameTemplate<'_> {}
// impl Partial for IndexTemplate<'_> {}

// #[derive(Template)]
// #[template(path = "layout.html")]
// struct Layout<'a> {
//     inner: Box<dyn Partial + 'a>,
// }
