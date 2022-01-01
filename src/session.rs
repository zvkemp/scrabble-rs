use axum::extract::{FromRequest, RequestParts};
use axum::http::{Request, StatusCode};
use axum::response::Redirect;
use axum::{async_trait, http};
use cookie::{Cookie, CookieJar, Key};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower::{Layer, Service};
use tracing::{debug, error, info};

use crate::users::User;

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Session {
    pub user_id: Option<i64>,
}

impl From<User> for Session {
    fn from(user: User) -> Self {
        Self {
            user_id: Some(user.id),
        }
    }
}

impl From<&User> for Session {
    fn from(user: &User) -> Self {
        Self {
            user_id: Some(user.id),
        }
    }
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
pub(crate) struct ExtractCookiesLayer;

#[derive(Debug, Clone)]
pub(crate) struct ExtractCookiesMiddleware<S> {
    service: S,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractSessionLayer;

impl<S, B> Service<Request<B>> for ExtractCookiesMiddleware<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        debug!("ExtractCookiesMiddleware");
        let (mut parts, body) = req.into_parts();

        let cookie_header: String = parts
            .headers
            .get(http::header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string())
            .unwrap_or_default();

        let mut jar = CookieJar::new();

        for cookie in cookie_header.split("; ") {
            tracing::debug!("attempting to parse {:?}", cookie);
            if !cookie.is_empty() {
                jar.add_original(
                    cookie
                        .parse()
                        .map_err(|e| {
                            error!("{:?}", e);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })
                        .unwrap(), // FIXME
                )
            }
        }

        parts.extensions.insert(jar);

        self.service.call(Request::from_parts(parts, body))
    }
}

impl<S> Layer<S> for ExtractCookiesLayer {
    type Service = ExtractCookiesMiddleware<S>;

    fn layer(&self, service: S) -> Self::Service {
        ExtractCookiesMiddleware { service }
    }
}

impl<S> Layer<S> for ExtractSessionLayer {
    type Service = ExtractSessionMiddleware<S>;

    fn layer(&self, service: S) -> Self::Service {
        ExtractSessionMiddleware { service }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExtractSessionMiddleware<S> {
    service: S,
}

impl<S, B> Service<Request<B>> for ExtractSessionMiddleware<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        debug!("ExtractSessionMiddleware");
        let (mut head, body) = req.into_parts();

        let jar: &CookieJar = head.extensions.get().unwrap();
        // .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        let key = Key::from(SECRET.as_bytes());

        let session: Session = match jar.private(&key).get(SESSION_COOKIE_NAME) {
            Some(cookie) => serde_json::from_str(cookie.value()).unwrap(),
            None => Session::default(),
        };

        head.extensions.insert(session);
        self.service.call(Request::from_parts(head, body))
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
