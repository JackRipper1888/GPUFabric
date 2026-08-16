package support

import (
	"bytes"
	"crypto/ecdsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"io"
	"net/http"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"
)

func TestLiveSupportRenderAndHSMContract(t *testing.T) {
	baseURL := strings.TrimRight(os.Getenv("E2E_SUPPORT_LIVE_URL"), "/")
	token := os.Getenv("E2E_SUPPORT_LIVE_TOKEN")
	if baseURL == "" || token == "" {
		t.Skip("set E2E_SUPPORT_LIVE_URL and E2E_SUPPORT_LIVE_TOKEN")
	}
	client := &http.Client{Timeout: 90 * time.Second}
	const expectedPDFText = "算力资产技术预评估报告 暂无上报"
	html := `<!doctype html><html><head><meta charset="utf-8"><title>Local E2E</title></head><body><h1>NON-PRODUCTION ASSESSMENT</h1><p>Chromium render contract.</p><p>` + expectedPDFText + `</p></body></html>`
	htmlHash := sha256.Sum256([]byte(html))
	renderBody, _ := json.Marshal(map[string]any{
		"reportId": "AER-LIVE-SUPPORT", "html": html,
		"htmlSha256": hex.EncodeToString(htmlHash[:]),
	})
	var render struct {
		Success bool `json:"success"`
		Data    struct {
			PDFBase64 string `json:"pdfBase64"`
			PDFSHA256 string `json:"pdfSha256"`
		} `json:"data"`
	}
	callLiveSupport(t, client, token, baseURL+"/internal/v1/pdf-renders", renderBody, &render)
	pdf, err := base64.StdEncoding.DecodeString(render.Data.PDFBase64)
	if err != nil {
		t.Fatal(err)
	}
	pdfHash := sha256.Sum256(pdf)
	if !render.Success || !bytes.HasPrefix(pdf, []byte("%PDF-")) ||
		render.Data.PDFSHA256 != hex.EncodeToString(pdfHash[:]) {
		t.Fatalf("invalid live render response: success=%v bytes=%d hash=%s", render.Success, len(pdf), render.Data.PDFSHA256)
	}
	verifyLivePDFText(t, pdf, expectedPDFText)

	signingDigest := sha256.Sum256([]byte("local-e2e-independent-signing-check"))
	signBody, _ := json.Marshal(map[string]any{
		"reportId":     "AER-LIVE-SUPPORT",
		"digestSha256": hex.EncodeToString(signingDigest[:]),
		"digestBase64": base64.StdEncoding.EncodeToString(signingDigest[:]),
	})
	var signed struct {
		Success bool            `json:"success"`
		Data    ReportSignature `json:"data"`
	}
	callLiveSupport(t, client, token, baseURL+"/internal/v1/report-signatures", signBody, &signed)
	verifyLiveSignature(t, signed.Data, signingDigest[:])
}

func verifyLivePDFText(t *testing.T, pdf []byte, expected string) {
	t.Helper()
	file, err := os.CreateTemp(t.TempDir(), "live-render-*.pdf")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.Write(pdf); err != nil {
		file.Close()
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	extracted, err := exec.Command("pdftotext", "-layout", file.Name(), "-").CombinedOutput()
	if err != nil {
		t.Fatalf("extract rendered PDF text: %v: %s", err, extracted)
	}
	if !strings.Contains(string(extracted), expected) {
		t.Fatalf("rendered PDF is missing CJK text %q; extracted text: %q", expected, extracted)
	}
}

func callLiveSupport(t *testing.T, client *http.Client, token, endpoint string, body []byte, target any) {
	t.Helper()
	request, err := http.NewRequest(http.MethodPost, endpoint, bytes.NewReader(body))
	if err != nil {
		t.Fatal(err)
	}
	request.Header.Set("Authorization", "Bearer "+token)
	request.Header.Set("Content-Type", "application/json")
	response, err := client.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	encoded, err := io.ReadAll(io.LimitReader(response.Body, 40<<20))
	if err != nil {
		t.Fatal(err)
	}
	if response.StatusCode != http.StatusOK {
		t.Fatalf("%s returned %d: %s", endpoint, response.StatusCode, encoded)
	}
	if err := json.Unmarshal(encoded, target); err != nil {
		t.Fatal(err)
	}
}

func verifyLiveSignature(t *testing.T, signature ReportSignature, digest []byte) {
	t.Helper()
	if signature.SchemaVersion != reportSignatureSchema ||
		signature.Algorithm != "ECDSA-P256-SHA256" || len(signature.CertificateChain) != 2 {
		t.Fatalf("invalid signature envelope: %+v", signature)
	}
	certificates := make([]*x509.Certificate, 0, len(signature.CertificateChain))
	for _, encoded := range signature.CertificateChain {
		block, _ := pem.Decode([]byte(encoded))
		if block == nil {
			t.Fatal("invalid certificate PEM")
		}
		certificate, err := x509.ParseCertificate(block.Bytes)
		if err != nil {
			t.Fatal(err)
		}
		certificates = append(certificates, certificate)
	}
	roots := x509.NewCertPool()
	roots.AddCert(certificates[1])
	if _, err := certificates[0].Verify(x509.VerifyOptions{
		Roots: roots, KeyUsages: []x509.ExtKeyUsage{x509.ExtKeyUsageCodeSigning},
	}); err != nil {
		t.Fatal(err)
	}
	publicKey, ok := certificates[0].PublicKey.(*ecdsa.PublicKey)
	if !ok {
		t.Fatal("leaf certificate is not ECDSA")
	}
	encodedSignature, err := base64.StdEncoding.DecodeString(signature.Signature)
	if err != nil || !ecdsa.VerifyASN1(publicKey, digest, encodedSignature) {
		t.Fatal("detached HSM signature did not verify")
	}
	chainHash := sha256.Sum256([]byte(strings.Join(signature.CertificateChain, "\n")))
	if signature.CertificateChainSHA256 != hex.EncodeToString(chainHash[:]) ||
		signature.SigningDigestSHA256 != hex.EncodeToString(digest) {
		t.Fatal("signature envelope hashes do not match")
	}
}
