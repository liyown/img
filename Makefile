.PHONY: build test lint fmt install cli-package desktop desktop-preview desktop-bundle desktop-test desktop-package desktop-release
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
cli-package:
	./scripts/package-cli.sh
desktop-bundle:
	./desktop/bundle.sh
desktop: desktop-bundle
	./target/Img.app/Contents/MacOS/img-desktop
desktop-preview: desktop-bundle
	./target/Img.app/Contents/MacOS/img-desktop --reference
desktop-test:
	cargo test --locked -p img-desktop
desktop-package:
	./desktop/package.sh --unsigned
desktop-release:
	./desktop/package.sh
