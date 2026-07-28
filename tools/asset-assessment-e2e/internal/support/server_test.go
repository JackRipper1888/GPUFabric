package support

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

type fakeRenderer struct {
	pdf []byte
	err error
}

func (renderer fakeRenderer) Render(context.Context, string, string) ([]byte, error) {
	return renderer.pdf, renderer.err
}

type fakeReportSigner struct {
	signature ReportSignature
	err       error
}

func (signer fakeReportSigner) Sign(context.Context, string) (ReportSignature, error) {
	return signer.signature, signer.err
}

func (signer fakeReportSigner) PublicInfo() TrustInfo {
	return TrustInfo{Mode: "test", Production: false, KeyVersion: "test-key-v1"}
}

func TestHTTPServerRenderAndSignContracts(t *testing.T) {
	const token = "local-test-support-token-32-bytes-minimum"
	pdf := []byte("%PDF-1.7\nfixture\n%%EOF")
	signer := fakeReportSigner{signature: ReportSignature{
		SchemaVersion: reportSignatureSchema, Algorithm: "ECDSA-P256-SHA256",
		KeyVersion: "test-key-v1",
	}}
	server := NewHTTPServer(token, 4096, fakeRenderer{pdf: pdf}, signer, nil, fakeMalwareScanner{}, NewCallbackSink(token))

	html := "<html><body>report</body></html>"
	htmlDigest := sha256.Sum256([]byte(html))
	body, _ := json.Marshal(map[string]any{
		"reportId": "AER-1", "html": html, "htmlSha256": hex.EncodeToString(htmlDigest[:]),
	})
	response := callSupport(server, http.MethodPost, "/internal/v1/pdf-renders", body, token)
	if response.Code != http.StatusOK {
		t.Fatalf("render returned %d: %s", response.Code, response.Body.String())
	}
	var renderEnvelope struct {
		Data struct {
			PDFBase64 string `json:"pdfBase64"`
			PDFSHA256 string `json:"pdfSha256"`
		} `json:"data"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &renderEnvelope); err != nil {
		t.Fatal(err)
	}
	pdfDigest := sha256.Sum256(pdf)
	if renderEnvelope.Data.PDFBase64 != base64.StdEncoding.EncodeToString(pdf) ||
		renderEnvelope.Data.PDFSHA256 != hex.EncodeToString(pdfDigest[:]) {
		t.Fatalf("unexpected render envelope: %+v", renderEnvelope)
	}

	digest := sha256.Sum256([]byte("sign me"))
	signBody, _ := json.Marshal(map[string]any{
		"reportId": "AER-1", "digestSha256": hex.EncodeToString(digest[:]),
		"digestBase64": base64.StdEncoding.EncodeToString(digest[:]),
	})
	response = callSupport(server, http.MethodPost, "/internal/v1/report-signatures", signBody, token)
	if response.Code != http.StatusOK || !strings.Contains(response.Body.String(), "test-key-v1") {
		t.Fatalf("sign returned %d: %s", response.Code, response.Body.String())
	}
}

func TestHTTPServerFailsClosedOnAuthHashAndUnknownFields(t *testing.T) {
	const token = "local-test-support-token-32-bytes-minimum"
	server := NewHTTPServer(token, 4096, fakeRenderer{pdf: []byte("%PDF-1.7\n%%EOF")}, fakeReportSigner{}, nil, fakeMalwareScanner{}, NewCallbackSink(token))
	body := []byte(`{"reportId":"AER-1","html":"x","htmlSha256":"` + strings.Repeat("0", 64) + `"}`)
	response := callSupport(server, http.MethodPost, "/internal/v1/pdf-renders", body, "wrong-token")
	if response.Code != http.StatusUnauthorized {
		t.Fatalf("expected auth rejection, got %d", response.Code)
	}
	response = callSupport(server, http.MethodPost, "/internal/v1/pdf-renders", body, token)
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected hash rejection, got %d: %s", response.Code, response.Body.String())
	}
	body = []byte(`{"reportId":"AER-1","html":"x","htmlSha256":"` + strings.Repeat("0", 64) + `","secret":"x"}`)
	response = callSupport(server, http.MethodPost, "/internal/v1/pdf-renders", body, token)
	if response.Code != http.StatusBadRequest {
		t.Fatalf("expected unknown-field rejection, got %d", response.Code)
	}
}

func callSupport(server *HTTPServer, method, path string, body []byte, token string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+token)
	request.Header.Set("Content-Type", "application/json")
	response := httptest.NewRecorder()
	server.Handler().ServeHTTP(response, request)
	return response
}
