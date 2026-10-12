.PHONY: build-linux-ubuntu20 docker-image-ubuntu20

# Build and run the Ubuntu 20.04-based Rust build container.
# Usage:
#   make build-linux-ubuntu20        # builds using ancillary/build-ubuntu20.sh (default cargo build --release --locked)
#   make docker-image-ubuntu20       # only builds the Docker image

docker-image-ubuntu20:
	docker build -t ubuntu20-rust-builder -f ancillary/Dockerfile.ubuntu20 .

build-linux-ubuntu20: docker-image-ubuntu20
	@echo "Running build script ancillary/build-ubuntu20.sh"
	sh ancillary/build-ubuntu20.sh
