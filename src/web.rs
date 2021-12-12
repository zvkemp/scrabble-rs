use std::sync::{Arc, Mutex};

use askama::Template;
use async_session::{CookieStore, SessionStore};
use axum::extract::{Extension, Form, FromRequest, Path, RequestParts, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{async_trait, http};
use axum::{AddExtensionLayer, Router};
use axum_channels::{registry::Registry, ConnFormat};
use serde::Deserialize;
use sqlx::PgPool;

use crate::users::User;

#[derive(Deserialize, Debug)]
struct SignUp {
    username: String,
    password: String,
}

pub fn app(registry: Arc<Mutex<Registry>>, pool: PgPool, store: CookieStore) -> Router {
    Router::new()
        .route("/simple/websocket", get(ws_handler))
        .route("/", get(index))
        .route("/play/:game_id/:player", get(show_game))
        .route("/js/index.js", get(assets::index_js))
        .route("/js/index.js.map", get(assets::index_js_map))
        .route("/css/styles.css", get(assets::css))
        .layer(AddExtensionLayer::new(registry))
        .layer(AddExtensionLayer::new(pool))
        .layer(AddExtensionLayer::new(store))
}

async fn sign_up(Form(signup): Form<SignUp>) -> impl IntoResponse {
    dbg!(signup);
    "Ok"
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Extension(registry): Extension<Arc<Mutex<Registry>>>,
    Extension(_pg_pool): Extension<PgPool>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        axum_channels::handle_connect(socket, ConnFormat::Phoenix, registry)
    })
}

async fn show_game(Path((game_id, player)): Path<(String, String)>) -> Html<String> {
    let template = GameTemplate {
        game_id: game_id.as_str(),
        player: player.as_str(),
        token: "fixme",
    };
    Html(template.render().unwrap())
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

async fn index() -> Html<String> {
    let template = IndexTemplate { name: "world" };
    Html(template.render().unwrap())
}

mod assets {
    pub async fn index_js() -> &'static str {
        include_str!("../assets/index.js")
    }

    pub async fn index_js_map() -> &'static str {
        include_str!("../assets/index.js.map")
    }

    pub async fn css() -> &'static str {
        include_str!("../assets/index.css")
    }
}

enum UserFromSession {
    User(User),
    None,
}

#[async_trait]
impl<B> FromRequest<B> for UserFromSession
where
    B: Send,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let Extension(store) = Extension::<CookieStore>::from_request(req)
            .await
            .expect("CookieStore not found");

        let headers = req.headers().unwrap();

        match headers
            .get(http::header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string())
        {
            Some(cookie) => {
                let session = store.load_session(cookie).await.unwrap();

                todo!()
            }
            None => todo!(),
        }
    }
}
