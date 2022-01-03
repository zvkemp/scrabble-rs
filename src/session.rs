use std::collections::hash_map::DefaultHasher;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::async_trait;
use axum::extract::{FromRequest, RequestParts};
use axum::http::{Request, StatusCode, Uri};
use axum::response::{Redirect, Response};
use cookie::{Cookie, CookieJar, Key};
use parking_lot::Mutex;
use pin_project::pin_project;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::hash::{Hash, Hasher};
use tower::{Layer, Service};
use tower_cookies::Cookies;
use tracing::debug;

use crate::users::User;

#[derive(Hash, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Session {
    pub user_id: Option<i64>,
    #[serde(default = "new_csrf_token")]
    csrf_token: String,
    #[serde(default)]
    login_redirect: Option<String>,
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
            csrf_token: new_csrf_token(),
            login_redirect: None,
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

        let session = req.extensions().unwrap().get::<SessionManager>().unwrap();
        let user_id = session.user_id();

        if user_id.is_none() {
            return Err(redirect_to_login(req, &session));
        }

        User::find(user_id.unwrap(), pool)
            .await
            .map(CurrentUser)
            .map_err(|_| redirect_to_login(req, &session))
    }
}

fn redirect_to_login<B>(req: &RequestParts<B>, session: &SessionManager) -> Redirect {
    session.set_login_redirect(Some(req.uri().to_string()));

    Redirect::temporary("/login".parse().unwrap())
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
    inner: Arc<Mutex<Session>>,
    hash: u64,
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

        if this.session.has_changed() {
            let cookie = Cookie::build(
                SESSION_COOKIE_NAME,
                serde_json::to_string(&this.session.as_json()).unwrap(),
            )
            .max_age(Duration::from_secs(31536000).try_into().unwrap())
            .path("/")
            .finish();
            // FIXME: only if changed
            let jar = this.cookies.private(key());

            jar.add(cookie);
        }

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
        let (mut head, body) = req.into_parts();

        let cookies: Cookies = head.extensions.get().cloned().unwrap();
        // .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        let key = Key::from(SECRET.as_bytes());

        let mut session_was_new = false;
        let session: Session = match cookies.private(&key).get(SESSION_COOKIE_NAME) {
            Some(cookie) => serde_json::from_str(cookie.value()).unwrap(),
            None => {
                session_was_new = true;
                Session::new()
            }
        };

        let mut session_manager = SessionManager::new(session);
        if session_was_new {
            session_manager.hash = 0; // force cookie to be set
        }

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
        let hash = Self::hash_session(&session);

        Self {
            inner: Arc::new(Mutex::new(session)),
            hash,
        }
    }

    pub fn hash_session(session: &Session) -> u64 {
        let mut hasher = DefaultHasher::new();
        session.hash(&mut hasher);
        hasher.finish()
    }

    pub fn as_json(&self) -> serde_json::Value {
        let inner = self.inner.lock();
        inner.as_json()
    }

    pub(crate) fn set_user_id(&self, id: Option<i64>) {
        let mut inner = self.inner.lock();
        inner.user_id = id;
    }

    pub(crate) fn user_id(&self) -> Option<i64> {
        self.inner.lock().user_id
    }

    pub(crate) fn current_hash(&self) -> u64 {
        let locked = self.inner.lock();
        Self::hash_session(&locked)
    }

    pub(crate) fn has_changed(&self) -> bool {
        self.current_hash() != self.hash
    }

    pub(crate) fn set_login_redirect(&self, login_redirect: Option<String>) {
        self.inner.lock().login_redirect = login_redirect;
    }

    pub(crate) fn take_login_redirect(&self) -> Option<String> {
        self.inner.lock().login_redirect.take()
    }

    pub(crate) fn csrf_token(&self) -> String {
        self.inner.lock().csrf_token.clone()
    }
}
