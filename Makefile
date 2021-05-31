server-watch:
	cargo watch -w 'crates/server' -w 'crates/common' -s 'cargo run -p game_server -- --data 127.0.0.1:42424 --public 127.0.0.1:42424 --http 127.0.0.1:9000'