default: build

build:
  cargo b
  cargo +nightly clippy
  cargo t

build-release:
  cargo b --release --all-features

deploy-static:
  rsync -avz ./static do-zdv:/srv/vzdv/

deploy bin: build-release
  scp target/release/vzdv-{{bin}} do-zdv:/srv/vzdv/vzdv-{{bin}}.new
