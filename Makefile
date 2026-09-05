.PHONY: build test lint fmt install desktop desktop-preview desktop-bundle desktop-test
build:
	cargo build --locked --release -p img-cli
	mkdir -p bin
	cp target/release/img bin/img
test:
	cargo test --locked -p img-core -p img-cli
lint:
	cargo clippy --locked -p img-core -p img-cli --all-targets -- -D warnings
fmt:
	cargo fmt --all
install:
	cargo install --locked --path crates/img-cli --force
desktop-bundle:
	./desktop/bundle.sh
desktop: desktop-bundle
	./target/Img.app/Contents/MacOS/img-desktop
desktop-preview: desktop-bundle
	./target/Img.app/Contents/MacOS/img-desktop --reference
desktop-test:
	cargo test --locked -p img-desktop
