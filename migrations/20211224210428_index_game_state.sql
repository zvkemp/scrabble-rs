CREATE INDEX index_games_on_state ON games USING HASH ((data->'state'));
