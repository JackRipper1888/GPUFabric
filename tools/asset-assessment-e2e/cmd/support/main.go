package main

import (
	"context"
	"errors"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/gpunexus/gpufabric/tools/asset-assessment-e2e/internal/support"
)

func main() {
	config, err := support.LoadConfig()
	if err != nil {
		log.Fatal(err)
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	signer, err := support.InitializePKCS11Signer(ctx, config)
	if err != nil {
		log.Fatalf("initialize non-production HSM identity: %v", err)
	}
	malware := support.NewClamDClient(config.ClamAVAddr, config.ScanTimeout)
	ocr := support.NewCLIExtractor(config.PDFToTextPath, config.PDFToPPMPath, config.TesseractPath, config.ScanTimeout)
	scanner := support.NewEvidenceScanner(malware, ocr, config.AllowedDownloadHosts, config.MaxEvidenceBytes, config.ScanTimeout)
	renderer := support.NewChromiumRenderer(config.ChromiumPath, config.RenderTimeout)
	callback := support.NewCallbackSink(config.CallbackSecret)
	handler := support.NewHTTPServer(config.Token, config.MaxHTMLBytes, renderer, signer, scanner, malware, callback)
	server := &http.Server{
		Addr: config.Addr, Handler: handler.Handler(),
		ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 2 * time.Minute,
		WriteTimeout: 2 * time.Minute, IdleTimeout: 30 * time.Second,
	}
	go func() {
		<-ctx.Done()
		shutdownContext, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer shutdownCancel()
		_ = server.Shutdown(shutdownContext)
	}()
	log.Printf("local E2E support listening on %s (non-production)", config.Addr)
	if err := server.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
}
