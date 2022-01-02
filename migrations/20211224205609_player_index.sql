CREATE INDEX index_games_on_player_names ON games USING GIN ((data->'players') jsonb_path_ops);
