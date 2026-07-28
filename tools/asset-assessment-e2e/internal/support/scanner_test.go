package support

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"
	"time"
)

type fakeMalwareScanner struct {
	result  MalwareResult
	err     error
	pingErr error
}

func (scanner fakeMalwareScanner) Ping(context.Context) error { return scanner.pingErr }
func (scanner fakeMalwareScanner) Scan(context.Context, []byte) (MalwareResult, error) {
	return scanner.result, scanner.err
}

type fakeOCRExtractor struct {
	text string
	err  error
}

func (extractor fakeOCRExtractor) Extract(context.Context, []byte, string) (string, error) {
	return extractor.text, extractor.err
}

func TestEvidenceScannerValidatesBytesAndRunsOCR(t *testing.T) {
	content := []byte("%PDF-1.7\nlocal fixture\n%%EOF")
	source := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Method != http.MethodGet {
			t.Fatalf("unexpected method %s", request.Method)
		}
		_, _ = writer.Write(content)
	}))
	defer source.Close()
	parsed, _ := url.Parse(source.URL)
	digest := sha256.Sum256(content)
	scanner := NewEvidenceScanner(
		fakeMalwareScanner{}, fakeOCRExtractor{text: "ownership invoice"},
		map[string]struct{}{parsed.Host: {}}, 1024, time.Second,
	)
	result, err := scanner.Scan(context.Background(), ScanRequest{
		JobID: "scan-1", DownloadMethod: http.MethodGet, DownloadURL: source.URL,
		RequiredHeaders: map[string]string{"Host": parsed.Host},
		ExpectedSHA256:  hex.EncodeToString(digest[:]), ExpectedContentType: "application/pdf",
		ExpectedContentLength: int64(len(content)), FileName: "invoice.pdf",
	})
	if err != nil {
		t.Fatal(err)
	}
	ocrHash := sha256.Sum256([]byte("ownership invoice"))
	if result.Status != "clean" || result.DetectedContentType != "application/pdf" ||
		result.SHA256 != hex.EncodeToString(digest[:]) ||
		result.OCRSHA256 != hex.EncodeToString(ocrHash[:]) {
		t.Fatalf("unexpected scan result: %+v", result)
	}
}

func TestEvidenceScannerReturnsInfectedAndRejectsIntegrityMismatch(t *testing.T) {
	content := []byte("%PDF-1.7\nEICAR\n%%EOF")
	source := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		_, _ = writer.Write(content)
	}))
	defer source.Close()
	parsed, _ := url.Parse(source.URL)
	digest := sha256.Sum256(content)
	scanner := NewEvidenceScanner(
		fakeMalwareScanner{result: MalwareResult{Infected: true, Signature: "Eicar-Signature"}},
		fakeOCRExtractor{text: "must not run"}, map[string]struct{}{parsed.Host: {}}, 1024, time.Second,
	)
	request := ScanRequest{
		JobID: "scan-infected", DownloadMethod: http.MethodGet, DownloadURL: source.URL,
		ExpectedSHA256: hex.EncodeToString(digest[:]), ExpectedContentType: "application/pdf",
		ExpectedContentLength: int64(len(content)), FileName: "infected.pdf",
	}
	result, err := scanner.Scan(context.Background(), request)
	if err != nil || result.Status != "infected" || result.MalwareSignature != "Eicar-Signature" ||
		result.OCRSHA256 != "" {
		t.Fatalf("unexpected infected result: %+v %v", result, err)
	}
	request.ExpectedSHA256 = hex.EncodeToString(make([]byte, sha256.Size))
	if _, err := scanner.Scan(context.Background(), request); err == nil {
		t.Fatal("expected SHA-256 mismatch rejection")
	}
	request.ExpectedSHA256 = hex.EncodeToString(digest[:])
	request.RequiredHeaders = map[string]string{"Host": "untrusted.invalid"}
	if _, err := scanner.Scan(context.Background(), request); err == nil {
		t.Fatal("expected signed Host mismatch rejection")
	}
	request.RequiredHeaders = nil
	request.DownloadURL = "http://untrusted.invalid/object"
	if _, err := scanner.Scan(context.Background(), request); err == nil {
		t.Fatal("expected untrusted host rejection")
	}
}
