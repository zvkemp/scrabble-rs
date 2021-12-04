use askama::Template;
use axum::{
    extract::{Extension, Path, WebSocketUpgrade},
    response::{Html, IntoResponse},
    routing::get,
    AddExtensionLayer, Router,
};
use axum_channels::{
    channel::{self, ChannelBehavior},
    message::{DecoratedMessage, Message},
    registry::Registry,
    ConnFormat,
};
use scrabble::{Game, Player};
use std::sync::{Arc, Mutex};

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

#[derive(Clone, Debug)]
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
