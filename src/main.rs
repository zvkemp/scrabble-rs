use askama::Template;
use axum::{
    extract::{Extension, Path, Query, WebSocketUpgrade},
    response::{Html, IntoResponse},
    routing::get,
    AddExtensionLayer, Router,
};
use axum_channels::{
    channel::{self, ChannelBehavior},
    message::{DecoratedMessage, Message, MessageKind, MessageReply},
    registry::Registry,
    ConnFormat,
};
use scrabble::{Game, Player};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tracing::{debug, error};

use crate::scrabble::Turn;

mod scrabble;

#[tokio::main]
async fn main() {
    // tracing_subscriber::fmt()
    //     .with_env_filter(EnvFilter::default())
    //     .with_writer(std::io::stdout)
    //     .initracing_subscriber::fmt::init();t();
    tracing_subscriber::fmt::init();

    let registry = Arc::new(Mutex::new(Registry::default()));
    let mut locked = registry.lock().unwrap();
    locked.register_behavior("game".to_string(), Box::new(GameChannel::new()));

    drop(locked);

    let app = Router::new()
        .route("/simple/websocket", get(handler))
        .route("/", get(index))
        .route("/play/:game_id/:player", get(show_game))
        .route("/js/index.js", get(index_js))
        .route("/js/index.js.map", get(index_js_map))
        .route("/css/styles.css", get(css))
        .layer(AddExtensionLayer::new(registry));

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
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        axum_channels::handle_connect(socket, ConnFormat::Phoenix, registry.clone())
    })
}

#[derive(Clone, Debug)]
struct GameChannel {
    pub(crate) game: Game,
}

impl GameChannel {
    pub fn new() -> Self {
        let game = Game::testing();

        GameChannel { game }
    }
}

impl GameChannel {
    fn play(&mut self, payload: serde_json::Value) -> Result<(), scrabble::Error> {
        let turn = payload.try_into()?;
        self.game.play(turn)?;

        Ok(())
    }
}

impl ChannelBehavior for GameChannel {
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
                        payload: json!(null),
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
                match message.inner.event.as_str() {
                    "player-state" => {
                        let payload = self.game.player_state(0); // FIXME: use real conn state index
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

    fn handle_join(&mut self, message: &DecoratedMessage) -> Result<(), channel::Error> {
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

        if let Some(socket) = message.ws_reply_to.as_ref() {
            let state = MessageReply::Event {
                event: "player-state".to_string(),
                payload: json!({
                    "game": self.game,
                    "rack": self.game.rack(player_index).map_err(|e| channel::Error::Other(e.to_string()))?,
                    "remaining": self.game.remaining_tiles(player_index)
                }),
                channel_id: message.channel_id().clone(),
            };

            let _ = socket.send(state);
        };

        Ok(())
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
