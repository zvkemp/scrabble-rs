use axum::{async_trait, http};
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
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tracing::{debug, error};
use users::User;

use crate::session::Session;

mod dictionary;
mod scrabble;
mod session;
mod users;
mod web;

// TODOs:
// blanks aren't playable yet
// allow spectators
// UI bugs (delete key erases played tiles)
// scores rendered in reverse

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();
    tracing_subscriber::fmt::init();

    dictionary::dictionary().await;

    let database_url = std::env::var("DATABASE_URL").unwrap();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .unwrap();

    // FIXME: internalize the Arc/Mutex
    let registry = Arc::new(Mutex::new(Registry::default()));
    let mut locked = registry.lock().unwrap();
    let game_channel = GameChannel::new(pool.clone(), "_template_".parse().unwrap());
    locked.register_template("game".to_string(), game_channel);

    drop(locked);

    let app = web::app(registry, pool);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let socket_addr = SocketAddr::new("0.0.0.0".parse().unwrap(), port.parse().unwrap());

    // FIXME use PORT
    axum::Server::bind(&socket_addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

struct PlayerIndex(usize);

#[derive(Debug)]
struct GameChannel {
    pub(crate) game: Option<Game>,
    pub(crate) socket_state: HashMap<Token, http::Extensions>,
    pub(crate) pg_pool: PgPool,
}

impl GameChannel {
    pub fn new(pg_pool: PgPool, channel_id: ChannelId) -> Self {
        GameChannel {
            game: None,
            socket_state: HashMap::new(),
            pg_pool,
        }
    }

    async fn play(&mut self, payload: serde_json::Value) -> Result<(), scrabble::Error> {
        let turn = payload.try_into()?;
        let game = self.game.as_mut().unwrap();

        game.play(turn).await?;
        self.save_state().await?;

        Ok(())
    }

    async fn save_state(&mut self) -> Result<(), scrabble::Error> {
        match self.game.as_mut().unwrap().persist(&self.pg_pool).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("error saving game state; e={:?}", e);

                Err(e)
            }
        }
    }
}

#[async_trait]
impl Channel for GameChannel {
    async fn handle_message(&mut self, message: &DecoratedMessage) -> Option<Message> {
        match &message.inner.kind {
            MessageKind::Event => match message.inner.event.as_str() {
                "start" => {
                    let _ = self.game.as_mut().unwrap().start();
                    let _ = self.save_state().await;

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
                    match self.play(message.inner.payload.clone()).await {
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
                            let msg = format!("{:?}", e);

                            Some(Message {
                                kind: MessageKind::Push,
                                channel_id: message.channel_id().clone(),
                                msg_ref: message.msg_ref.clone(),
                                join_ref: None,
                                payload: serde_json::json!({
                                    "message": msg,
                                }),
                                event: "error".into(),
                                channel_sender: None,
                            })
                        }
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }

    async fn handle_out(&mut self, message: &DecoratedMessage) -> Option<Message> {
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
                        let payload = self.game.as_ref().unwrap().player_state(index.0);
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
        if self.game.is_none() {
            let game = Game::fetch(message.channel_id().clone(), &self.pg_pool).await;
            debug!("setting up game {:?}...", message.channel_id());
            self.game = Some(game);
        }

        debug!("{:?}", message);
        let token = message
            .inner
            .payload
            .get("token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| channel::Error::Other("token not found".into()))
            .map(|token| Session::read_token(token.to_string()))?;

        let session = token.ok_or_else(|| channel::Error::Other("token was not valid".into()))?;

        let user = User::find(session.user_id.unwrap(), &self.pg_pool)
            .await // damn it
            .unwrap(); // FIXME: unwrap

        let player = Player(user.username);

        let player_index = self
            .game
            .as_mut()
            .unwrap()
            .add_player(player)
            .map_err(|e| channel::Error::Join {
                // FIXME: allow spectators?
                reason: format!("{:?}", e),
            })?;

        let _ = self.save_state().await;

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

    async fn handle_leave(
        &mut self,
        message: &DecoratedMessage,
    ) -> axum_channels::channel::Result<Option<Message>> {
        self.socket_state.remove(&message.token);
        Ok(None)
    }

    async fn handle_presence(
        &mut self,
        channel_id: &ChannelId,
        presence: &Presence,
    ) -> axum_channels::channel::Result<Option<Message>> {
        let mut online = HashSet::new();

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
    fn new_channel(&self, channel_id: ChannelId) -> Box<dyn Channel> {
        Box::new(GameChannel::new(self.pg_pool.clone(), channel_id))
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
