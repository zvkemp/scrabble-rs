use std::time::Duration;

use askama::Template;
use axum::extract::{ws::WebSocketUpgrade, Extension, Form, Path};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::{AddExtensionLayer, Router};
use axum_channels::registry::{RegistryMessage, RegistrySender};
use axum_channels::ConnFormat;
use cookie::{Cookie, Key};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::oneshot;
use tower_cookies::{CookieManagerLayer, Cookies};
use tracing::debug;

use crate::session::{self, CurrentUser, SessionManager, SessionManagerLayer};
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

pub fn app(registry: RegistrySender, pool: PgPool) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/sign_up", get(new_registration))
        .route("/register", post(create_registration))
        .route("/login", get(new_login))
        .route("/login", post(create_login))
        .route("/simple/websocket", get(ws_handler))
        .route("/play/:game_id", get(show_game))
        .route("/rand_game", get(rand_game))
        .route("/debug/registry", get(debug_registry))
        .layer(
            tower::ServiceBuilder::new()
                .layer(CookieManagerLayer::new())
                .layer(SessionManagerLayer)
                .layer(AddExtensionLayer::new(registry))
                .layer(AddExtensionLayer::new(pool)),
        )
        // FIXME: use tower-http's ServeFile (https://github.com/tokio-rs/axum/blob/e0082a3f87a266fe6832fcd634b5e6c295daddf6/axum/src/docs/routing/route.md)
        .route("/js/index.js", get(assets::index_js))
        .route("/js/index.js.map", get(assets::index_js_map))
        .route("/css/styles.css", get(assets::css))
}

async fn new_login() -> Html<String> {
    let template = NewLoginTemplate {
        csrf_token: "FIXME",
    };
    Html(template.render().unwrap())
}

async fn create_login(
    Form(login): Form<Login>,
    Extension(pool): Extension<PgPool>,
    Extension(session): Extension<SessionManager>,
) -> Result<Redirect, Error> {
    let user = User::find_by_username_and_password(&login.username, &login.password, &pool)
        .await
        .map_err(Error::User)?;

    session.inner.lock().user_id = Some(user.id);

    // let session = Session::from(user);
    // let cookie = Cookie::build(
    //     session::SESSION_COOKIE_NAME,
    //     serde_json::to_string(&session).unwrap(),
    // )
    // .max_age(Duration::from_secs(31536000).try_into().unwrap())
    // .finish();

    // let key = Key::from(session::SECRET.as_bytes());
    // let private = jar.private(&key);

    // private.add(cookie);

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

async fn debug_registry(_: CurrentUser, Extension(registry): Extension<RegistrySender>) -> String {
    let (tx, rx) = oneshot::channel();
    registry.send(RegistryMessage::Debug(tx));
    rx.await.unwrap()
}

enum Error {
    PasswordConfirmation,
    #[allow(dead_code)]
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

// FIXME: move boilerplate into lib
async fn ws_handler(
    ws: WebSocketUpgrade,
    Extension(registry): Extension<RegistrySender>,
    Extension(_pg_pool): Extension<PgPool>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        axum_channels::handle_connect(socket, ConnFormat::Phoenix, registry)
    })
}

async fn show_game(Path(game_id): Path<String>, CurrentUser(user): CurrentUser) -> Html<String> {
    let session = session::Session::from(&user);
    let token = session.token();

    let template = GameTemplate {
        game_id: game_id.as_str(),
        token: token.as_str(),
        player: user.username.as_str(),
    };

    Html(template.render().unwrap())
}

#[derive(Template)]
#[template(path = "game.html")]
struct GameTemplate<'a> {
    game_id: &'a str,
    token: &'a str,
    player: &'a str,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    info: &'a str,
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

async fn index(Extension(session): Extension<SessionManager>) -> Html<String> {
    let info = format!("{:?}", session.inner.lock());
    let template = IndexTemplate {
        info: info.as_str(),
    };
    Html(template.render().unwrap())
}

async fn rand_game(_: CurrentUser) -> Redirect {
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(30)
        .map(char::from)
        .collect();

    Redirect::to(format!("/play/{}", rand_string).parse().unwrap())
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
