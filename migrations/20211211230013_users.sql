CREATE TABLE users (
  id BIGSERIAL PRIMARY KEY,
  username VARCHAR NOT NULL,
  hashed_password VARCHAR NOT NULL
);

CREATE UNIQUE INDEX index_users_on_username ON users(username);
