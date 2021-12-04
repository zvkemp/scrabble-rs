use askama::Template;
use axum::{
    extract::{Extension, Path, WebSocketUpgrade},
    response::{Html, IntoResponse},
    routing::get,
    AddExtensionLayer, Router,
};
use axum_channels::{
    channel::{self, ChannelBehavior},
    message::{DecoratedMessage, Message, MessageReply},
    registry::Registry,
    types::Token,
    ConnFormat,
};
use scrabble::{Game, Player};
use std::{
    fmt::Display,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::UnboundedSender;
use tracing_subscriber::EnvFilter;

mod scrabble;

#[tokio::main]
async fn main() {
    // tracing_subscriber::fmt()
    //     .with_env_filter(EnvFilter::default())
    //     .with_writer(std::io::stdout)
    //     .initracing_subscriber::fmt::init();t();
    tracing_subscriber::fmt::init();

    let registry = Arc::new(Mutex::new(Registry::default()));
    let lobby = Box::new(Lobby(registry.clone()));

    let mut locked = registry.lock().unwrap();

    locked.add_channel("lobby".to_string(), lobby);

    drop(locked);

    let app = Router::new()
        .route("/simple/websocket", get(handler))
        .route("/", get(index))
        .route("/play/:game_id", get(show_game))
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

async fn show_game(Path(game_id): Path<String>) -> Html<String> {
    let template = GameTemplate {
        game_id: game_id.as_str(),
        player: "fixme",
        token: "fixme",
    };
    Html(template.render().unwrap())
}

async fn handler(
    ws: WebSocketUpgrade,
    Extension(registry): Extension<Arc<Mutex<Registry>>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        axum_channels::handle_connect(socket, ConnFormat::Message, registry.clone())
    })
}

// FIXME: lobby should be rolled into Registry;
// i.e. a join request to a specifically-formatted key should
// optionally spawn a new channel topic;
// i.e.
// join game:foo should use the GameChannel behavior for messages to foo.
// Rather than dithering about with broadcast subscriptions, it may be beneficial to
// have a key-value fanout rather then direct mpsc/broadcast connections;
// this refactor will also help in the future where a distributed pubsub may be desireable.
// (see Phoenix.PubSub.Redis/PG2, etc)
#[derive(Debug)]
struct Lobby(Arc<Mutex<Registry>>);

impl ChannelBehavior for Lobby {
    fn handle_message(&mut self, message: &DecoratedMessage) -> Option<Message> {
        match &message.inner {
            Message::Channel { text, .. } => {
                return self.handle_message_inner(
                    message.token,
                    message.reply_to.as_ref(),
                    text.to_string(),
                );
            }
            // these should never happen
            Message::Join { .. } => todo!(),
            Message::Leave { .. } => todo!(),
            Message::DidJoin { .. } => todo!(),
            Message::Reply(_) => todo!(),
            Message::Broadcast(_) => todo!(),
        };

        None
    }
}

impl Lobby {
    fn handle_message_inner(
        &mut self,
        token: Token,
        reply_to: Option<&UnboundedSender<Message>>,
        text: String,
    ) -> Option<Message> {
        let mut tokens = text.split_whitespace();
        match tokens.next() {
            Some("new_game") => {
                let name = tokens.next().unwrap(); // FIXME

                let mut locked = self.0.lock().unwrap();
                let game = Box::new(GameChannel::new());

                // FIXME: something something security
                locked.add_channel(name.to_string(), game);

                reply_to.map(|sender| {
                    sender.send(Message::Join {
                        channel_id: name.to_string(),
                    });
                });

                return Some(Message::Broadcast(format!("{:#?}", self)));
                // FIXME: ensure socket joins game!
            }
            _ => {
                eprintln!("unknown command {:?}", text);
                return None;
            }
        }
    }
}

struct GameChannel {
    pub(crate) game: Game,
}

impl GameChannel {
    pub fn new() -> Self {
        GameChannel {
            game: Game::default(),
        }
    }
}

impl ChannelBehavior for GameChannel {
    fn handle_message(&mut self, _message: &DecoratedMessage) -> Option<Message> {
        None
    }

    fn handle_join(&mut self, message: &DecoratedMessage) -> Result<(), channel::JoinError> {
        let player = Player(format!("{:?}", message.token));
        self.game
            .add_player(player)
            .map_err(|_| channel::JoinError::Unknown)?;

        println!("{:#?}", self.game);

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
