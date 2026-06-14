test:
    make test

polish:
    make fmt && make clippy

build:
    make build

build-release:
    make release

clean:
    make clean

ci: polish test
