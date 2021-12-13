use sqlx::{query, Executor, FromRow, PgExecutor, Transaction};

#[derive(FromRow, Debug)]
pub struct User {
    pub id: i64,
    pub username: String,
    hashed_password: String,
}

#[derive(Debug)]
pub enum Error {
    Unknown,
    Bcrypt(bcrypt::BcryptError),
    Sqlx(sqlx::Error),
    NotFound,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl User {
    pub async fn find<'a, E>(id: i64, db: E) -> Result<User, Error>
    where
        E: PgExecutor<'a>,
    {
        let user: User =
            sqlx::query_as("SELECT id, username, hashed_password from users WHERE id = $1;")
                .bind(id)
                .fetch_one(db)
                .await
                .map_err(Error::Sqlx)?;

        Ok(user)
    }

    pub async fn find_by_username<'a, E>(
        username: &str,
        db: E, //Transaction<'_, sqlx::Postgres>,
    ) -> Result<User, Error>
    where
        E: PgExecutor<'a>,
    {
        let user: User =
            sqlx::query_as("SELECT id, username, hashed_password from users WHERE username = $1;")
                .bind(username)
                .fetch_one(db)
                .await
                .map_err(Error::Sqlx)?;

        Ok(user)
    }

    // FIXME: return error on incorrect password?
    pub async fn find_by_username_and_password<'a, E>(
        username: &str,
        password: &str,
        db: E,
    ) -> Result<User, Error>
    where
        E: PgExecutor<'a>,
    {
        let user = Self::find_by_username(username, db).await?;

        dbg!(bcrypt::verify(password, &user.hashed_password))
            .map_err(Error::Bcrypt)
            .and_then(|res| res.then(|| user).ok_or(Error::NotFound))
    }

    pub async fn create<'a, E>(username: &str, password: &str, tx: E) -> Result<i64, Error>
    where
        E: PgExecutor<'a>,
    {
        let hashed_password = bcrypt::hash(password, bcrypt_cost()).map_err(Error::Bcrypt)?;

        let result = sqlx::query!(
            "INSERT INTO users (username, hashed_password) VALUES ($1, $2) RETURNING id;",
            username,
            hashed_password
        )
        .fetch_one(tx)
        .await
        .map_err(Error::Sqlx)?;

        Ok(result.id)
    }
}

#[cfg(not(test))]
fn bcrypt_cost() -> u32 {
    // bcrypt::DEFAULT_COST
    4
}

#[cfg(test)]
fn bcrypt_cost() -> u32 {
    4
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{postgres::PgPoolOptions, PgPool};

    async fn test_pool() -> PgPool {
        PgPoolOptions::new()
            .max_connections(5)
            .connect("postgres://localhost/scrabble_rs_test")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_insert_and_find_user() {
        let pool = test_pool().await;

        let mut tx = pool.begin().await.unwrap();

        User::create("test_user_3", "password", &mut tx)
            .await
            .unwrap();

        let user = User::find_by_username("test_user_3", &mut tx)
            .await
            .unwrap();

        assert_ne!(user.hashed_password, "password");

        assert!(bcrypt::verify("password", &user.hashed_password).unwrap());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_by_password() {
        let pool = test_pool().await;

        let mut tx = pool.begin().await.unwrap();

        User::create("test_user_4", "password", &mut tx)
            .await
            .unwrap();

        let user = User::find_by_username_and_password("test_user_4", "wrong", &mut tx).await;

        assert!(user.is_err());

        let user = User::find_by_username_and_password("test_user_4", "password", &mut tx)
            .await
            .unwrap();

        tx.rollback().await.unwrap();
    }
}
