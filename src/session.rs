use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::async_trait;
use axum::extract::{FromRequest, RequestParts};
use axum::http::{Request, StatusCode};
use axum::response::{Redirect, Response};
use cookie::{Cookie, CookieJar, Key};
use parking_lot::Mutex;
use pin_project::pin_project;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tower::{Layer, Service};
use tower_cookies::Cookies;
use tracing::debug;

use crate::users::User;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Session {
    pub user_id: Option<i64>,
    #[serde(default = "default_data")]
    pub data: serde_json::Value,
    #[serde(default = "new_csrf_token")]
    csrf_token: String,
}

impl From<User> for Session {
    fn from(user: User) -> Self {
        let mut session = Session::new();
        session.user_id = Some(user.id);
        session
    }
}

impl From<&User> for Session {
    fn from(user: &User) -> Self {
        let mut session = Session::new();
        session.user_id = Some(user.id);
        session
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            user_id: None,
            data: default_data(),
            csrf_token: new_csrf_token(),
        }
    }

    pub fn as_json(&self) -> serde_json::Value {
        json!(self)
    }
}

fn new_csrf_token() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

fn default_data() -> serde_json::Value {
    serde_json::json!({})
}

impl Session {
    // essentially a key-less encrypted cookie value;
    // there's probably another way to do this
    pub fn token(&self) -> String {
        let mut jar = CookieJar::new();
        let mut private = jar.private_mut(key());
        let cookie = Cookie::new(SESSION_COOKIE_NAME, serde_json::to_string(self).unwrap());
        private.add(cookie);

        jar.get(SESSION_COOKIE_NAME).unwrap().value().to_string()
    }

    pub fn read_token(token: String) -> Option<Session> {
        let mut jar = CookieJar::new();
        jar.add_original(Cookie::new(SESSION_COOKIE_NAME, token));
        let value = jar
            .private(key())
            .get(SESSION_COOKIE_NAME)?
            .value()
            .to_string();

        serde_json::from_str(&value).ok()?
    }
}

fn key() -> &'static Key {
    &*KEY
}

pub static SESSION_COOKIE_NAME: &str = "_scrabble_rs_session";

lazy_static::lazy_static! {
    pub static ref SECRET: String = std::env::var("SECRET_KEY_BASE").unwrap_or_else(|_|
                "FIXME-the-is-the-default-development-key-and-should-not-be-used!".to_string());
    pub static ref KEY: Key = Key::from(secret_key_base());
}

fn secret_key_base() -> &'static [u8] {
    SECRET.as_bytes()
}

#[async_trait]
impl<B> FromRequest<B> for Session
where
    B: Send,
{
    type Rejection = StatusCode;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        req.extensions_mut()
            .unwrap()
            .remove()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

pub(crate) struct CurrentUser(pub User);

#[async_trait]
impl<B> FromRequest<B> for CurrentUser
where
    B: Send,
{
    type Rejection = Redirect;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let pool = req.extensions().unwrap().get::<PgPool>().unwrap();

        let user_id = req
            .extensions()
            .unwrap()
            .get::<Session>()
            .and_then(|session| session.user_id);

        // FIXME: include login_redirect in session
        if user_id.is_none() {
            return Err(Redirect::to("/login".parse().unwrap()));
        }

        User::find(user_id.unwrap(), pool)
            .await
            .map(CurrentUser)
            .map_err(|_| Redirect::to("/login".parse().unwrap()))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SessionManagerLayer;

impl<S> Layer<S> for SessionManagerLayer {
    type Service = SessionManagerMiddleware<S>;

    fn layer(&self, service: S) -> Self::Service {
        SessionManagerMiddleware { service }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SessionManagerMiddleware<S> {
    service: S,
}

// FIXME: make generic for Serialize
// FIXME: track changes
#[derive(Clone, Debug)]
pub struct SessionManager {
    pub inner: Arc<Mutex<Session>>,
}

impl SessionManager {
    pub fn as_json(&self) -> serde_json::Value {
        let inner = self.inner.lock();
        inner.as_json()
    }
}

#[pin_project]
pub struct SessionManagerFuture<F> {
    #[pin]
    wrapped: F,
    session: SessionManager,
    cookies: Cookies,
}

impl<F, B, E> Future for SessionManagerFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = match this.wrapped.poll(cx) {
            Poll::Ready(result) => result,
            Poll::Pending => return Poll::Pending,
        }?;

        let cookie = Cookie::build(
            SESSION_COOKIE_NAME,
            serde_json::to_string(&this.session.as_json()).unwrap(),
        )
        .max_age(Duration::from_secs(31536000).try_into().unwrap())
        .finish();
        // FIXME: only if changed
        let jar = this.cookies.private(key());

        jar.add(dbg!(cookie));

        Poll::Ready(Ok(result))
    }
}

impl<S, B, Res> Service<Request<B>> for SessionManagerMiddleware<S>
where
    S: Service<Request<B>, Response = Response<Res>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = SessionManagerFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        debug!("ExtractSessionMiddleware");
        let (mut head, body) = req.into_parts();

        let cookies: Cookies = head.extensions.get().cloned().unwrap();
        // .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        let key = Key::from(SECRET.as_bytes());

        let session: Session = match cookies.private(&key).get(SESSION_COOKIE_NAME) {
            Some(cookie) => serde_json::from_str(cookie.value()).unwrap(),
            None => Session::new(),
        };

        let session_manager = SessionManager::new(session);

        head.extensions.insert(session_manager.clone());

        SessionManagerFuture {
            wrapped: self.service.call(Request::from_parts(head, body)),
            session: session_manager,
            cookies,
        }
    }
}

#[cfg(test)]
mod tests {
    use cookie::{Cookie, CookieJar};

    use super::*;

    #[test]
    fn test_private_jar() {
        let value = "secret-thing-here";
        let cookie = Cookie::new(SESSION_COOKIE_NAME, value);
        let key = Key::from(SECRET.as_bytes());

        let mut jar = CookieJar::new();

        jar.private_mut(&key).add(cookie);

        let encrypted = jar.get(SESSION_COOKIE_NAME).unwrap();

        let mut new_jar = CookieJar::new();
        new_jar.add_original(encrypted.clone());

        let decrypted = new_jar.private(&key).get(SESSION_COOKIE_NAME).unwrap();

        dbg!(encrypted.to_string());
        dbg!(decrypted.to_string());
    }
}
impl SessionManager {
    pub(crate) fn new(session: Session) -> Self {
        Self {
            inner: Arc::new(Mutex::new(session)),
        }
    }
}
