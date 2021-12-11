use askama::Template;
use axum::{
    extract::{Extension, Path, Query, WebSocketUpgrade},
    http,
    response::{Html, IntoResponse},
    routing::get,
    AddExtensionLayer, Router,
};
use axum_channels::{
    channel::{self, Channel, Presence},
    message::{DecoratedMessage, Message, MessageKind, MessageReply},
    registry::Registry,
    types::{ChannelId, Token},
    ConnFormat,
};
use scrabble::{Game, Player};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tracing::{debug, error};

use crate::scrabble::Turn;

mod scrabble;
mod users;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // FIXME: internalize the Arc/Mutex
    let registry = Arc::new(Mutex::new(Registry::default()));
    let mut locked = registry.lock().unwrap();
    locked.register_template("game".to_string(), GameChannel::default());

    drop(locked);

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect("postgres://localhost/scrabble_rs")
        .await
        .unwrap();

    let row: (i64,) = sqlx::query_as("SELECT $1")
        .bind(150_i64)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(row.0, 150);

    let app = Router::new()
        .route("/simple/websocket", get(handler))
        .route("/", get(index))
        .route("/play/:game_id/:player", get(show_game))
        .route("/js/index.js", get(index_js))
        .route("/js/index.js.map", get(index_js_map))
        .route("/css/styles.css", get(css))
        .layer(AddExtensionLayer::new(registry))
        .layer(AddExtensionLayer::new(pool));

    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn index() -> Html<String> {
    let template = IndexTemplate { name: "world" };
    Html(template.render().unwrap())
}

async fn index_js() -> &'static str {
    include_str!("../assets/index.js")
}

async fn index_js_map() -> &'static str {
    include_str!("../assets/index.js.map")
}

async fn css() -> &'static str {
    include_str!("../assets/index.css")
}

async fn show_game(Path((game_id, player)): Path<(String, String)>) -> Html<String> {
    let template = GameTemplate {
        game_id: game_id.as_str(),
        player: player.as_str(),
        token: "fixme",
    };
    Html(template.render().unwrap())
}

async fn handler(
    ws: WebSocketUpgrade,
    Extension(registry): Extension<Arc<Mutex<Registry>>>,
    Extension(_pg_pool): Extension<PgPool>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        axum_channels::handle_connect(socket, ConnFormat::Phoenix, registry)
    })
}

struct PlayerIndex(usize);

#[derive(Debug, Default)]
struct GameChannel {
    pub(crate) game: Game,
    pub(crate) socket_state: HashMap<Token, http::Extensions>,
}

impl GameChannel {
    pub fn new() -> Self {
        let game = Game::default();

        GameChannel {
            game,
            socket_state: HashMap::new(),
        }
    }

    fn play(&mut self, payload: serde_json::Value) -> Result<(), scrabble::Error> {
        let turn = payload.try_into()?;
        self.game.play(turn)?;

        Ok(())
    }
}

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

    fn handle_join(
        &mut self,
        message: &DecoratedMessage,
    ) -> Result<Option<Message>, channel::Error> {
        debug!("{:?}", message);
        let player = Player(
            message
                .inner
                .payload
                .get("player")
                .and_then(|p| p.as_str())
                .ok_or_else(|| channel::Error::Other("player not found".into()))?
                .into(),
        );

        let player_index = self
            .game
            .add_player(player)
            .map_err(|e| channel::Error::Join {
                reason: format!("{:?}", e),
            })?;

        let state = self.socket_state.entry(message.token).or_default();
        state.insert(PlayerIndex(player_index));

        // if let Some(socket) = message.ws_reply_to.as_ref() {
        //     // FIXME: this should broadcast
        //     let state = MessageReply::Event {
        //         event: "player-state".to_string(),
        //         payload: self.game.player_state(player_index),
        //         channel_id: message.channel_id().clone(),
        //     };

        //     let _ = socket.send(state);
        // };

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
        // let mut map = HashMap::new();
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

#[derive(Template)]
#[template(path = "game.html")]
struct GameTemplate<'a> {
    game_id: &'a str,
    player: &'a str,
    token: &'a str,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    name: &'a str,
}

// trait Partial: Template + Display {}

// impl Partial for GameTemplate<'_> {}
// impl Partial for IndexTemplate<'_> {}

// #[derive(Template)]
// #[template(path = "layout.html")]
// struct Layout<'a> {
//     inner: Box<dyn Partial + 'a>,
// }
