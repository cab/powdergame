server-watch:
	cargo watch -w 'crates/server' -w 'crates/common' -s 'cargo run -p game_server -- --data 0.0.0.0:8099 --public 0.0.0.0:8098 --http 0.0.0.0:9000'