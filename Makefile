all: build serve

build:
	cargo build --release --target wasm32-unknown-unknown
	wasm-bindgen --out-name app \
	  --out-dir www/target \
	  --target web target/wasm32-unknown-unknown/release/main.wasm

deploy:
	echo "At this point, we expect the ship repo to exist locally."
	echo "Get the repo with:"
	echo "git clone git@github.com:antoineMoPa/claydash-ship.git"
	cp -r www/* claydash-ship/
	echo "Commit and push ../claydash-ship to release!"

serve:
	python3 -m http.server --directory www 3001


doc:
	cargo doc --open

run-native:
	cargo run
