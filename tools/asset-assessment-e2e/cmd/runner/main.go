package main

import (
	"context"
	"log"
	"os"
	"os/signal"
	"syscall"

	"github.com/gpunexus/gpufabric/tools/asset-assessment-e2e/internal/e2erunner"
)

func main() {
	config, err := e2erunner.LoadConfig()
	if err != nil {
		log.Fatal(err)
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	result, err := e2erunner.Run(ctx, config)
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("local full-chain E2E passed: %s", result)
}
