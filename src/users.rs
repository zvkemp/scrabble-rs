use sqlx::{FromRow, PgPool};

#[derive(FromRow)]
pub struct User {
    id: u32,
    username: String,
    hashed_password: String,
}

pub enum Error {
    Unknown,
}

impl User {
    pub async fn find_by_username(username: &str, pool: &PgPool) -> Result<User, sqlx::Error> {
        let user: User =
            sqlx::query_as("SELECT id, username, hashed_password from users WHERE username = $1;")
                .bind(username)
                .fetch_one(pool)
                .await?;

        Ok(user)
    }

    pub async fn create(username: &str, password: &str, pool: &PgPool) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO users VALUES ($1, $2)")
            .bind(username)
            .bind(password)
            .fetch(pool);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
        t
    }
}
