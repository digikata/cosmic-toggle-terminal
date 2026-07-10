
_default:
	just -l

deploy:
	cargo build --release
	cp target/release/cosmic-toggle-terminal ~/bin
	cargo clean
