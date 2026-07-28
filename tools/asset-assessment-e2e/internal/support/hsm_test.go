package support

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"encoding/asn1"
	"encoding/pem"
	"math/big"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestNormalizeECDSASignatureAcceptsRawAndDER(t *testing.T) {
	raw := make([]byte, 64)
	raw[31] = 1
	raw[63] = 2
	encoded, err := normalizeECDSASignature(raw)
	if err != nil {
		t.Fatal(err)
	}
	var values struct{ R, S *big.Int }
	if _, err := asn1.Unmarshal(encoded, &values); err != nil {
		t.Fatal(err)
	}
	if values.R.Int64() != 1 || values.S.Int64() != 2 {
		t.Fatalf("unexpected signature values: %s/%s", values.R, values.S)
	}
	repeated, err := normalizeECDSASignature(encoded)
	if err != nil {
		t.Fatal(err)
	}
	if string(repeated) != string(encoded) {
		t.Fatal("DER signature was changed")
	}
}

func TestPublishTrustRootWritesOnlyPublicCertificate(t *testing.T) {
	privateKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	template := &x509.Certificate{
		SerialNumber: big.NewInt(1),
		NotBefore:    time.Now().Add(-time.Hour),
		NotAfter:     time.Now().Add(time.Hour),
		IsCA:         true,
		KeyUsage:     x509.KeyUsageCertSign,
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &privateKey.PublicKey, privateKey)
	if err != nil {
		t.Fatal(err)
	}
	encoded := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der})
	path := filepath.Join(t.TempDir(), "public", "root.pem")
	if err := publishTrustRoot(path, encoded); err != nil {
		t.Fatal(err)
	}
	actual, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(actual) != string(encoded) {
		t.Fatal("published trust root content changed")
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0o644 {
		t.Fatalf("trust root mode = %o, want 644", info.Mode().Perm())
	}
	if block, rest := pem.Decode(actual); block == nil || block.Type != "CERTIFICATE" || len(rest) != 0 {
		t.Fatal("published trust root contains non-certificate material")
	}
}

func TestNormalizeECDSASignatureRejectsMalformedValue(t *testing.T) {
	if _, err := normalizeECDSASignature([]byte{1, 2, 3}); err == nil {
		t.Fatal("expected malformed signature rejection")
	}
}

func TestParseP256PublicKeySupportsSPKIAndECPoint(t *testing.T) {
	privateKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	spki, err := x509.MarshalPKIXPublicKey(&privateKey.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := parseP256PublicKey(spki)
	if err != nil || parsed.X.Cmp(privateKey.X) != 0 || parsed.Y.Cmp(privateKey.Y) != 0 {
		t.Fatalf("parse SPKI: %v", err)
	}
	point := elliptic.Marshal(elliptic.P256(), privateKey.X, privateKey.Y)
	octets, err := asn1.Marshal(point)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err = parseP256PublicKey(octets)
	if err != nil || parsed.X.Cmp(privateKey.X) != 0 || parsed.Y.Cmp(privateKey.Y) != 0 {
		t.Fatalf("parse EC point: %v", err)
	}
}
