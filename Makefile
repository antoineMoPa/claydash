all: build serve

build:
	cargo build --release --target wasm32-unknown-unknown
	wasm-bindgen --out-name app \
	  --out-dir www/target \
	  --target web target/wasm32-unknown-unknown/release/main.wasm

deploy:
	du -h target/wasm32-unknown-unknown/release/main.wasm
	cp -r www/* claydash-ship/


serve:
	python3 -m http.server --directory www 3001


doc:
	cargo doc --open

run-native:
	cargo run
