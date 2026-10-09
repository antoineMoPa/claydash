all: build serve

build:
	cargo build --release --target wasm32-unknown-unknown
	wasm-bindgen --out-name app \
	  --out-dir www/target \
	  --target web target/wasm32-unknown-unknown/release/claydash.wasm
	cp assets/icons/lucide/loader-circle.svg www/target/loader-circle.svg
	cp assets/poisson-mesh-worker.js www/poisson-mesh-worker.js
	cp assets/voxel-worker.js www/voxel-worker.js
	cp assets/poisson-encode-worker.js www/poisson-encode-worker.js

deploy:
	du -h target/wasm32-unknown-unknown/release/claydash.wasm
	cp -r www/* claydash-ship/


serve:
	python3 -m http.server --directory www 3001


doc:
	cargo doc --open

guide:
	scripts/capture-guide.sh

guide-check:
	scripts/capture-guide.sh --check

run-native:
	cargo run
