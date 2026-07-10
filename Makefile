BINARY := img
VERSION ?= dev
COMMIT ?= $(shell git rev-parse --short HEAD 2>/dev/null || printf unknown)
DATE ?= $(shell date -u +%Y-%m-%dT%H:%M:%SZ)
LDFLAGS := -s -w -X main.version=$(VERSION) -X main.commit=$(COMMIT) -X main.date=$(DATE)

GOVULNCHECK_VERSION ?= v1.6.0

.PHONY: build test lint fmt vuln install cross
build:
	mkdir -p bin
	go build -trimpath -ldflags "$(LDFLAGS)" -o bin/$(BINARY) ./cmd/img
test:
	go test -race ./...
lint:
	go vet ./...
fmt:
	gofmt -w cmd internal
vuln:
	go run golang.org/x/vuln/cmd/govulncheck@$(GOVULNCHECK_VERSION) ./...
install:
	go install -trimpath -ldflags "$(LDFLAGS)" ./cmd/img
cross:
	mkdir -p dist
	GOOS=darwin GOARCH=amd64 go build -trimpath -ldflags "$(LDFLAGS)" -o dist/img-darwin-amd64 ./cmd/img
	GOOS=darwin GOARCH=arm64 go build -trimpath -ldflags "$(LDFLAGS)" -o dist/img-darwin-arm64 ./cmd/img
	GOOS=linux GOARCH=amd64 go build -trimpath -ldflags "$(LDFLAGS)" -o dist/img-linux-amd64 ./cmd/img
	GOOS=linux GOARCH=arm64 go build -trimpath -ldflags "$(LDFLAGS)" -o dist/img-linux-arm64 ./cmd/img
	GOOS=windows GOARCH=amd64 go build -trimpath -ldflags "$(LDFLAGS)" -o dist/img-windows-amd64.exe ./cmd/img
