use std::sync::{Arc, Mutex};

use askama::Template;
use axum::extract::{ws::WebSocketUpgrade, Extension, Form, FromRequest, Path, RequestParts};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{async_trait, http, Json};
use axum::{AddExtensionLayer, Router};
use axum_channels::{registry::Registry, ConnFormat};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tracing::debug;

use crate::session::{ExtractCookies, Session};
use crate::users;
use crate::users::User;

#[derive(Deserialize, Debug)]
struct Registration {
    username: String,
    password: String,
    password_confirmation: String,
    _csrf_token: String,
}

#[derive(Deserialize, Debug)]
struct Login {
    username: String,
    password: String,
}

pub fn app(registry: Arc<Mutex<Registry>>, pool: PgPool) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/sign_up", get(new_registration))
        .route("/register", post(create_registration))
        .route("/login", get(new_login))
        .route("/login", post(create_login))
        .route("/simple/websocket", get(ws_handler))
        .route("/play/:game_id/:player", get(show_game))
        .route("/js/index.js", get(assets::index_js))
        .route("/js/index.js.map", get(assets::index_js_map))
        .route("/css/styles.css", get(assets::css))
        .layer(AddExtensionLayer::new(registry))
        .layer(AddExtensionLayer::new(pool))
    // .layer(AddExtensionLayer::new(store))
}

async fn new_login() -> Html<String> {
    let template = NewLoginTemplate {
        csrf_token: "FIXME",
    };
    Html(template.render().unwrap())
}

// struct Session(async_store::Session);

async fn create_login(
    Form(login): Form<Login>,
    Extension(pool): Extension<PgPool>,
    // Extension(store): Extension<CookieStore>,
) -> Result<Redirect, Error> {
    let user = User::find_by_username_and_password(&login.username, &login.password, &pool)
        .await
        .map_err(Error::User)?;

    dbg!(user);

    Ok(Redirect::to("/".parse().unwrap()))
}

async fn create_registration(
    Form(registration): Form<Registration>,
    Extension(pool): Extension<PgPool>,
) -> Result<Html<String>, Error> {
    debug!("create_registration");
    // FIXME: verify CSRF token

    let id = registration.commit(pool).await?;
    debug!("registered");

    Ok(Html(format!("user_id={}", id)))
}

enum Error {
    PasswordConfirmation,
    Csrf,
    User(users::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        debug!("IntoResponse for Error");
        let (status, error_message) = match self {
            Error::PasswordConfirmation => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Password does not match confirmation".to_string(),
            ),
            Error::Csrf => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid CSRF token".to_string(),
            ),
            Error::User(e) => (StatusCode::UNPROCESSABLE_ENTITY, format!("{:?}", e)),
        };

        let body = Json(json!({
            "error": error_message,
        }));
        debug!("IntoResponse for Error finished");

        (status, body).into_response()
    }
}

impl Registration {
    fn validate(&self) -> Result<(), Error> {
        (self.password == self.password_confirmation)
            .then(|| ())
            .ok_or(Error::PasswordConfirmation)?;

        self.verify_csrf()?;

        Ok(())
    }

    pub async fn commit(&self, pool: PgPool) -> Result<i64, Error> {
        debug!("validate");
        self.validate()?;

        debug!("starting create");
        User::create(&self.username, &self.password, &pool)
            .await
            .map_err(Error::User)
    }

    fn verify_csrf(&self) -> Result<(), Error> {
        // FIXME!

        Ok(())
    }
}

async fn new_registration() -> Html<String> {
    let template = NewRegistrationTemplate {
        csrf_token: "FIXME",
    };
    Html(template.render().unwrap())
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

async fn show_game(
    Path((game_id, player)): Path<(String, String)>,
    _: ExtractCookies,
    session: Session,
) -> Html<String> {
    dbg!(session);
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

#[derive(Template)]
#[template(path = "new_registration.html")]
struct NewRegistrationTemplate<'a> {
    csrf_token: &'a str,
}

#[derive(Template)]
#[template(path = "login.html")]
struct NewLoginTemplate<'a> {
    csrf_token: &'a str,
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
