package support

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"time"
)

type PDFRenderer interface {
	Render(ctx context.Context, reportID, html string) ([]byte, error)
}

type ChromiumRenderer struct {
	path    string
	timeout time.Duration
	runner  commandRunner
}

func NewChromiumRenderer(path string, timeout time.Duration) *ChromiumRenderer {
	return &ChromiumRenderer{path: path, timeout: timeout, runner: execCommandRunner{}}
}

func (renderer *ChromiumRenderer) Render(ctx context.Context, reportID, html string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(ctx, renderer.timeout)
	defer cancel()
	directory, err := os.MkdirTemp("", "assessment-render-")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(directory)
	htmlPath := filepath.Join(directory, "report.html")
	pdfPath := filepath.Join(directory, "report.pdf")
	if err := os.WriteFile(htmlPath, []byte(html), 0o600); err != nil {
		return nil, err
	}
	if _, err := renderer.runner.Run(ctx, nil, nil, renderer.path,
		"--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
		"--no-pdf-header-footer", "--allow-file-access-from-files",
		"--print-to-pdf="+pdfPath, "file://"+htmlPath); err != nil {
		return nil, err
	}
	pdf, err := os.ReadFile(pdfPath)
	if err != nil {
		return nil, err
	}
	if len(pdf) < 16 || !bytes.HasPrefix(pdf, []byte("%PDF-")) {
		return nil, errors.New("Chromium returned an invalid PDF")
	}
	return pdf, nil
}

func validateHTMLHash(html, expected string) bool {
	digest := sha256.Sum256([]byte(html))
	return hex.EncodeToString(digest[:]) == expected
}
