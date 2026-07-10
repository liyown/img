package main

import (
	"context"
	"os"
	"os/signal"
	"syscall"

	"github.com/liyown/img/internal/cli"
)

var version = "dev"
var commit = "unknown"
var date = "unknown"

func main() {
	cli.Version = version
	cli.Commit = commit
	cli.BuildDate = date
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	os.Exit(cli.New().Run(ctx, os.Args[1:]))
}
