all: build serve

build:
	cargo build --release --target wasm32-unknown-unknown
	wasm-bindgen --out-name app \
	  --out-dir www/target \
	  --target web target/wasm32-unknown-unknown/release/main.wasm

	rm -rf claydash-ship/

	git clone git@github.com:antoineMoPa/claydash-ship.git

	rm -rf claydash-ship/*
	cp -r www/* claydash-ship/
	echo "Commit and push ../claydash-ship to release!"

serve:
	python3 -m http.server --directory www 3001


doc:
	cargo doc --open

run-native:
	cargo run
