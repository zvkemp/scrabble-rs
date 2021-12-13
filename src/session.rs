use axum::extract::{FromRequest, RequestParts};
use axum::http::StatusCode;
use axum::{async_trait, http};
use cookie::{Cookie, CookieJar, Key, PrivateJar};
use serde::{Deserialize, Serialize};
use tracing::error;

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

impl Session {
    // essentially a key-less encrypted cookie value;
    // there's probably another way to do this
    pub fn token(&self) -> String {
        let mut jar = CookieJar::new();
        let mut private = jar.private_mut(&key());
        let cookie = Cookie::new(SESSION_COOKIE_NAME, serde_json::to_string(self).unwrap());
        private.add(cookie);

        jar.get(SESSION_COOKIE_NAME).unwrap().value().to_string()
    }

    pub fn read_token(token: String) -> Option<Session> {
        let mut jar = CookieJar::new();
        jar.add_original(Cookie::new(SESSION_COOKIE_NAME, token));
        let value = jar
            .private(&key())
            .get(SESSION_COOKIE_NAME)?
            .value()
            .to_string();

        serde_json::from_str(&value).ok()?
    }
}

fn key() -> Key {
    Key::from(SECRET.as_bytes())
}

pub struct ExtractCookies;

pub static SESSION_COOKIE_NAME: &'static str = "_scrabble_rs_session";
pub static SECRET: &'static str =
    "FIXME-this-is-a-temporary-secret-and-needs-to-be-longer-until-64";

// inserts cookie jar into extensions
#[async_trait]
impl<B> FromRequest<B> for ExtractCookies
where
    B: Send,
{
    type Rejection = StatusCode;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let headers = req.headers().expect("headers have already been taken");

        let cookie_header: String = headers
            .get(http::header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string())
            .unwrap_or_default();

        let mut jar = CookieJar::new();

        for cookie in cookie_header.split("; ") {
            tracing::debug!("attempting to parse {:?}", cookie);
            if !cookie.is_empty() {
                jar.add_original(cookie.parse().map_err(|e| {
                    error!("{:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?)
            }
        }

        req.extensions_mut()
            .expect("extensions did not exist")
            .insert(jar);

        Ok(ExtractCookies)
    }
}

#[async_trait]
impl<B> FromRequest<B> for Session
where
    B: Send,
{
    type Rejection = StatusCode;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let jar: &CookieJar = req
            .extensions()
            .unwrap() // FIXME
            .get()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        let key = Key::from(SECRET.as_bytes());

        match jar.private(&key).get(SESSION_COOKIE_NAME) {
            Some(cookie) => {
                let session: Session = serde_json::from_str(cookie.value()).unwrap();

                Ok(session)
            }
            None => Ok(Session::default()),
        }
    }
}

// fn new_session_cookie(session: &Session) -> Result<Cookie,

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
