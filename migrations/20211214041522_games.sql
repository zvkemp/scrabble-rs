-- Add migration script here
CREATE TABLE games (
  id BIGSERIAL PRIMARY KEY,
  name VARCHAR NOT NULL,
  data JSONB
);

CREATE UNIQUE INDEX index_games_on_name ON games(name);
