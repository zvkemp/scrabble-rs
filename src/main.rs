use axum::{async_trait, http};
use axum_channels::{
    channel::{self, Channel, MessageContext, NewChannel, Presence},
    message::{self, Message, MessageKind},
    registry::Registry,
    types::{ChannelId, Token},
};
use scrabble::{Game, Player, TurnScore};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
};
use tracing::{debug, error, warn};
use users::User;

use crate::{scrabble::PlayerIndex, session::Session};

mod dictionary;
mod scrabble;
mod session;
mod users;
mod web;

// TODOs:
// allow spectators
// scores rendered in reverse

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();
    console_subscriber::Builder::default().init();

    dictionary::dictionary().await;

    let database_url = std::env::var("DATABASE_URL").unwrap();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .unwrap();

    let mut registry = Registry::default();
    let game_channel = GameChannel::new(pool.clone(), "_template_".parse().unwrap());
    registry.register_template("game", game_channel);

    let (registry_sender, _registry_handle) = registry.start();

    let app = web::app(registry_sender, pool);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let socket_addr = SocketAddr::new("0.0.0.0".parse().unwrap(), port.parse().unwrap());

    axum::Server::bind(&socket_addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

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

    fn propose(&self, payload: serde_json::Value) -> Result<TurnScore, scrabble::Error> {
        let turn = payload.try_into().map_err(|_| scrabble::Error::TurnParse)?;
        Ok(self.game.as_ref().unwrap().propose(&turn))
    }

    async fn play(
        &mut self,
        event: &str,
        payload: serde_json::Value,
        player_index: usize,
    ) -> Result<(), scrabble::Error> {
        let turn = payload.try_into()?;
        let game = self.game.as_mut().unwrap();

        if game.player_index != player_index {
            return Err(scrabble::Error::NotYourTurn);
        }

        let result = match event {
            "play" => game.play(turn).await,
            "swap" => game.swap(turn),
            "pass" => game.pass(),
            _ => {
                error!("unknown event {:?}", event);
                return Err(scrabble::Error::Unknown);
            }
        };

        // save state even if an error is returned
        self.save_state().await?;

        result
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

    fn broadcast_player_state(&self, channel_id: ChannelId) -> Message {
        message::broadcast_intercept(channel_id, "player-state".into(), Default::default())
    }
}

// FIXME: need a nicer way to declare messages
#[async_trait]
impl Channel for GameChannel {
    async fn handle_message(&mut self, message: &MessageContext) -> Option<Message> {
        match &message.inner.kind {
            MessageKind::Event => match message.inner.event.as_ref() {
                "start" => {
                    let _ = self.game.as_mut().unwrap().start();
                    let _ = self.save_state().await;

                    Some(self.broadcast_player_state(message.channel_id().clone()))
                }

                "play" | "swap" | "pass" => {
                    let index = self
                        .socket_state
                        .get(&message.token)
                        .unwrap()
                        .get::<PlayerIndex>()
                        .unwrap()
                        .0;

                    match self
                        .play(
                            message.inner.event.as_ref(),
                            message.inner.payload.clone(),
                            index,
                        )
                        .await
                    {
                        Ok(_) => Some(self.broadcast_player_state(message.channel_id().clone())),
                        Err(e) => {
                            error!("{:?}", e);
                            let msg = format!("{:?}", e);

                            match e {
                                scrabble::Error::TriesExhausted => {
                                    let mut reply =
                                        self.broadcast_player_state(message.channel_id().clone());

                                    let state = self.socket_state.entry(message.token).or_default();
                                    let player = state.get::<Player>();
                                    let info = message::broadcast(
                                        message.channel_id().clone(),
                                        "info".into(),
                                        json!({
                                            "message":
                                                format!(
                                                    "{:?} lost a turn due to illegal maneuvers!",
                                                    player
                                                )
                                        }),
                                    );

                                    message.push(info);

                                    dbg!(Some(reply))
                                }
                                _ => Some(message::push(
                                    message.channel_id().clone(),
                                    message.msg_ref.clone(),
                                    "error".into(),
                                    serde_json::json!({
                                        "message": msg,
                                    }),
                                )),
                            }
                        }
                    }
                }

                "proposed" => match self.propose(message.inner.payload.clone()) {
                    Ok(scores) => Some(message::push(
                        message.channel_id().clone(),
                        message.msg_ref.clone(),
                        "info".into(),
                        serde_json::json!({ "message": format!("{:?}", scores) }),
                    )),

                    Err(e) => Some(message::push(
                        message.channel_id().clone(),
                        message.msg_ref.clone(),
                        "error".into(),
                        serde_json::json!({ "message": format!("{:?}", e) }),
                    )),
                },

                other => {
                    warn!(
                        "unhandled message [{}]; payload={:?}",
                        other, message.inner.payload
                    );
                    None
                }
            },
            _ => None,
        }
    }

    async fn handle_out(&mut self, message: &MessageContext) -> Option<Message> {
        match &message.inner.kind {
            MessageKind::BroadcastIntercept => {
                let index = self
                    .socket_state
                    .get(&message.token)
                    .and_then(|entry| entry.get::<PlayerIndex>());

                // FIXME: ideally, we would be able to send
                // two messages in response to an action
                let flash = dbg!(message.inner.payload.get("message"));

                match message.inner.event.as_ref() {
                    "player-state" => {
                        let mut payload = self.game.as_ref().unwrap().player_state(index);
                        if let Some(flash) = flash {
                            payload["message"] = flash.clone();
                        }
                        let reply = message::push(
                            message.channel_id().clone(),
                            message.msg_ref.clone(),
                            message.inner.event.clone(),
                            payload,
                        );

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
        message: &MessageContext,
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

        match self.game.as_mut().unwrap().add_player(player.clone()) {
            Ok(player_index) => {
                let _ = self.save_state().await;
                let state = self.socket_state.entry(message.token).or_default();

                state.insert(PlayerIndex(player_index));
                state.insert(player);
            }

            Err(e) => {
                error!("{:?}", e);
            }
        }

        Ok(Some(
            self.broadcast_player_state(message.channel_id().clone()),
        ))
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

        let message = message::broadcast(
            channel_id.clone(),
            "presence".into(),
            serde_json::json!({ "online": online.iter().collect::<Vec<_>>() }),
        );

        Ok(Some(message))
    }

    async fn handle_leave(
        &mut self,
        message: &MessageContext,
    ) -> axum_channels::channel::Result<Option<Message>> {
        self.socket_state.remove(&message.token);
        Ok(None)
    }
}

impl NewChannel for GameChannel {
    fn new_channel(&self, channel_id: ChannelId) -> Box<dyn Channel> {
        Box::new(GameChannel::new(self.pg_pool.clone(), channel_id))
    }
}
